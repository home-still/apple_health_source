//! One-shot importer for USDA FoodData Central CSVs.
//!
//! Expected layout: `--dir <path>` pointing at an unzipped USDA CSV bundle
//! (SR Legacy or Foundation Foods) containing `food.csv`, `nutrient.csv`,
//! `food_nutrient.csv`, `food_portion.csv`, `measure_unit.csv`.
//!
//! Run twice (once per dataset) — `--reset` truncates first.
//!
//! `cargo run --bin import_usda -- --dir ./usda/sr_legacy --reset`
//! `cargo run --bin import_usda -- --dir ./usda/foundation`

use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use csv::{ReaderBuilder, StringRecord, Writer, WriterBuilder};
use sqlx::PgConnection;
use sqlx::postgres::PgPoolOptions;

#[derive(Parser, Debug)]
#[command(about = "Import USDA FoodData Central CSVs into the nutrition schema")]
struct Args {
    /// Directory containing the unzipped USDA CSV files.
    #[arg(long)]
    dir: PathBuf,
    /// TRUNCATE the nutrition tables before importing.
    #[arg(long, default_value_t = false)]
    reset: bool,
}

const FOOD_DATA_TYPES: &[&str] = &["sr_legacy_food", "foundation_food"];

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();

    let args = Args::parse();
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;

    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect(&database_url)
        .await?;

    let mut conn = pool.acquire().await?;

    if args.reset {
        tracing::info!("resetting nutrition tables");
        sqlx::query(
            "TRUNCATE TABLE nutrition.food_nutrients, nutrition.food_portions, \
             nutrition.foods, nutrition.measure_units, nutrition.nutrients RESTART IDENTITY CASCADE",
        )
        .execute(conn.as_mut())
        .await?;
    }

    import_nutrients(conn.as_mut(), &args.dir.join("nutrient.csv")).await?;
    import_measure_units(conn.as_mut(), &args.dir.join("measure_unit.csv")).await?;
    let fdc_ids = import_foods(conn.as_mut(), &args.dir.join("food.csv")).await?;
    import_food_nutrients(
        conn.as_mut(),
        &args.dir.join("food_nutrient.csv"),
        &fdc_ids,
    )
    .await?;
    import_food_portions(
        conn.as_mut(),
        &args.dir.join("food_portion.csv"),
        &fdc_ids,
    )
    .await?;

    tracing::info!("USDA import complete");
    Ok(())
}

/// Stream a CSV body (written in-memory by `writer_fn`) into Postgres via COPY.
async fn copy_csv(
    conn: &mut PgConnection,
    sql: &str,
    writer_fn: impl FnOnce(&mut Writer<Vec<u8>>) -> Result<()>,
) -> Result<u64> {
    let mut writer = WriterBuilder::new().has_headers(false).from_writer(Vec::new());
    writer_fn(&mut writer)?;
    let bytes = writer.into_inner()?;

    let mut copy = conn.copy_in_raw(sql).await?;
    copy.send(bytes.as_slice()).await?;
    let rows = copy.finish().await?;
    Ok(rows)
}

async fn import_nutrients(conn: &mut PgConnection, path: &Path) -> Result<()> {
    let sql = "COPY nutrition.nutrients(id, name, unit_name, nutrient_nbr, rank) \
               FROM STDIN WITH (FORMAT csv)";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, |r, header| {
            let id = field_i32(r, header, "id")?;
            let name = field_str(r, header, "name")?;
            let unit = field_str(r, header, "unit_name")?;
            let nbr = field_opt(r, header, "nutrient_nbr");
            let rank = field_opt_i32(r, header, "rank");
            w.write_record([
                &id.to_string(),
                name,
                unit,
                nbr.unwrap_or(""),
                &rank.map(|v| v.to_string()).unwrap_or_default(),
            ])?;
            Ok(())
        })
    })
    .await?;
    tracing::info!(rows, "imported nutrients");
    Ok(())
}

async fn import_measure_units(conn: &mut PgConnection, path: &Path) -> Result<()> {
    if !path.exists() {
        tracing::warn!(?path, "measure_unit.csv missing, skipping");
        return Ok(());
    }
    let sql = "COPY nutrition.measure_units(id, name) FROM STDIN WITH (FORMAT csv)";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, |r, header| {
            let id = field_i32(r, header, "id")?;
            let name = field_str(r, header, "name")?;
            w.write_record([&id.to_string(), name])?;
            Ok(())
        })
    })
    .await?;
    tracing::info!(rows, "imported measure_units");
    Ok(())
}

async fn import_foods(conn: &mut PgConnection, path: &Path) -> Result<HashSet<i32>> {
    let mut kept = HashSet::new();
    let sql = "COPY nutrition.foods(fdc_id, name, data_type, food_category_id, publication_date) \
               FROM STDIN WITH (FORMAT csv)";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, |r, header| {
            let data_type = field_str(r, header, "data_type")?;
            if !FOOD_DATA_TYPES.contains(&data_type) {
                return Ok(());
            }
            let fdc_id = field_i32(r, header, "fdc_id")?;
            let name = field_str(r, header, "description")?;
            let category = field_opt_i32(r, header, "food_category_id");
            let pub_date = field_opt(r, header, "publication_date").unwrap_or("");
            kept.insert(fdc_id);
            w.write_record([
                &fdc_id.to_string(),
                name,
                data_type,
                &category.map(|v| v.to_string()).unwrap_or_default(),
                pub_date,
            ])?;
            Ok(())
        })
    })
    .await?;
    tracing::info!(rows, "imported foods");
    Ok(kept)
}

async fn import_food_nutrients(
    conn: &mut PgConnection,
    path: &Path,
    fdc_ids: &HashSet<i32>,
) -> Result<()> {
    let sql = "COPY nutrition.food_nutrients(id, fdc_id, nutrient_id, amount) \
               FROM STDIN WITH (FORMAT csv)";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, |r, header| {
            let fdc_id = field_i32(r, header, "fdc_id")?;
            if !fdc_ids.contains(&fdc_id) {
                return Ok(());
            }
            let id = field_i64(r, header, "id")?;
            let nutrient_id = field_i32(r, header, "nutrient_id")?;
            let amount = field_opt_f32(r, header, "amount").unwrap_or(0.0);
            w.write_record([
                &id.to_string(),
                &fdc_id.to_string(),
                &nutrient_id.to_string(),
                &amount.to_string(),
            ])?;
            Ok(())
        })
    })
    .await?;
    tracing::info!(rows, "imported food_nutrients");
    Ok(())
}

async fn import_food_portions(
    conn: &mut PgConnection,
    path: &Path,
    fdc_ids: &HashSet<i32>,
) -> Result<()> {
    if !path.exists() {
        tracing::warn!(?path, "food_portion.csv missing, skipping");
        return Ok(());
    }
    let sql = "COPY nutrition.food_portions(id, fdc_id, amount, measure_unit_id, \
               portion_description, modifier, gram_weight) FROM STDIN WITH (FORMAT csv)";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, |r, header| {
            let fdc_id = field_i32(r, header, "fdc_id")?;
            if !fdc_ids.contains(&fdc_id) {
                return Ok(());
            }
            let id = field_i64(r, header, "id")?;
            let amount = field_opt_f32(r, header, "amount");
            let measure_unit_id = field_opt_i32(r, header, "measure_unit_id");
            let portion = field_opt(r, header, "portion_description").unwrap_or("");
            let modifier = field_opt(r, header, "modifier").unwrap_or("");
            let gram_weight = field_opt_f32(r, header, "gram_weight").unwrap_or(0.0);
            w.write_record([
                &id.to_string(),
                &fdc_id.to_string(),
                &amount.map(|v| v.to_string()).unwrap_or_default(),
                &measure_unit_id.map(|v| v.to_string()).unwrap_or_default(),
                portion,
                modifier,
                &gram_weight.to_string(),
            ])?;
            Ok(())
        })
    })
    .await?;
    tracing::info!(rows, "imported food_portions");
    Ok(())
}

fn each_record(
    path: &Path,
    mut per_row: impl FnMut(&StringRecord, &StringRecord) -> Result<()>,
) -> Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut rdr = ReaderBuilder::new().has_headers(true).flexible(true).from_reader(file);
    let header = rdr.headers()?.clone();
    let mut record = StringRecord::new();
    while rdr.read_record(&mut record)? {
        per_row(&record, &header)?;
    }
    Ok(())
}

fn column_index<'a>(header: &'a StringRecord, name: &str) -> Result<usize> {
    header
        .iter()
        .position(|h| h == name)
        .ok_or_else(|| anyhow!("missing column {name}"))
}

fn field_str<'r>(r: &'r StringRecord, header: &StringRecord, name: &str) -> Result<&'r str> {
    let idx = column_index(header, name)?;
    r.get(idx).ok_or_else(|| anyhow!("missing value {name}"))
}

fn field_opt<'r>(r: &'r StringRecord, header: &StringRecord, name: &str) -> Option<&'r str> {
    let idx = column_index(header, name).ok()?;
    r.get(idx).filter(|v| !v.is_empty())
}

fn field_i32(r: &StringRecord, header: &StringRecord, name: &str) -> Result<i32> {
    field_str(r, header, name)?.parse().with_context(|| format!("parse {name}"))
}

fn field_i64(r: &StringRecord, header: &StringRecord, name: &str) -> Result<i64> {
    field_str(r, header, name)?.parse().with_context(|| format!("parse {name}"))
}

fn field_opt_i32(r: &StringRecord, header: &StringRecord, name: &str) -> Option<i32> {
    field_opt(r, header, name).and_then(|v| v.parse().ok())
}

fn field_opt_f32(r: &StringRecord, header: &StringRecord, name: &str) -> Option<f32> {
    field_opt(r, header, name).and_then(|v| v.parse().ok())
}
