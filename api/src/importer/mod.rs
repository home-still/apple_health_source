//! USDA FoodData Central importer.
//!
//! Extracted into a library module so tests (and future batch jobs) can drive
//! the import without reaching into the binary's `main`. The `bin/import_usda`
//! entrypoint is a thin CLI wrapper around `run`.

use std::collections::HashSet;
use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use csv::{ReaderBuilder, StringRecord, Writer, WriterBuilder};
use sqlx::PgConnection;
use sqlx::PgPool;

use crate::nutrition::cache::NutritionCache;

#[derive(Debug, Clone)]
pub struct ImportArgs {
    /// Directory containing unzipped USDA CSV files.
    pub dir: PathBuf,
    /// `TRUNCATE` the nutrition tables before importing.
    pub reset: bool,
    /// Comma-separated list of USDA `data_type` values to keep. Empty list is
    /// rejected up front.
    pub data_types: Vec<String>,
    /// If `true`, skip rows that fail to parse instead of aborting.
    pub continue_on_error: bool,
    /// If `true`, don't import `nutrient.csv` / `measure_unit.csv`. Use this on
    /// the second/third dataset in a chained import (SR Legacy + Foundation)
    /// where the reference tables are already populated and re-import would
    /// collide on the `id` primary key.
    pub skip_reference: bool,
}

/// Run the full importer against the given pool. If `cache` is supplied,
/// `--reset` invalidates it so stale `fdc_id` mappings don't linger.
pub async fn run(args: ImportArgs, pool: &PgPool, cache: Option<&NutritionCache>) -> Result<()> {
    if args.data_types.is_empty() {
        bail!("--data-types must include at least one value (e.g. sr_legacy_food,foundation_food)");
    }
    tracing::info!(data_types = ?args.data_types, "filtering food.csv by data_type");

    let mut conn = pool.acquire().await?;

    if args.reset {
        tracing::info!("resetting nutrition tables");
        sqlx::query(
            "TRUNCATE TABLE nutrition.food_nutrients, nutrition.food_portions, \
             nutrition.foods, nutrition.measure_units, nutrition.nutrients RESTART IDENTITY CASCADE",
        )
        .execute(conn.as_mut())
        .await?;
        if let Some(c) = cache {
            c.invalidate_all().await;
        }
    }

    let ctx = RowCtx {
        continue_on_error: args.continue_on_error,
    };

    if args.skip_reference {
        tracing::info!("skipping nutrient.csv / measure_unit.csv (--skip-reference)");
    } else {
        import_nutrients(conn.as_mut(), &args.dir.join("nutrient.csv"), &ctx).await?;
        import_measure_units(conn.as_mut(), &args.dir.join("measure_unit.csv"), &ctx).await?;
    }
    let fdc_ids =
        import_foods(conn.as_mut(), &args.dir.join("food.csv"), &args.data_types, &ctx).await?;
    import_food_nutrients(
        conn.as_mut(),
        &args.dir.join("food_nutrient.csv"),
        &fdc_ids,
        &ctx,
    )
    .await?;
    import_food_portions(
        conn.as_mut(),
        &args.dir.join("food_portion.csv"),
        &fdc_ids,
        &ctx,
    )
    .await?;

    tracing::info!("USDA import complete");
    Ok(())
}

struct RowCtx {
    continue_on_error: bool,
}

/// Stream a CSV body (written in-memory by `writer_fn`) into Postgres via COPY.
///
/// Every COPY uses `NULL ''` so unquoted empty fields land in Postgres as NULL
/// rather than an empty string. This matters for columns like
/// `portion_description` and `modifier` where `"" .contains(s)` would
/// otherwise match every user query at lookup time.
async fn copy_csv(
    conn: &mut PgConnection,
    sql: &str,
    writer_fn: impl FnOnce(&mut Writer<Vec<u8>>) -> Result<()>,
) -> Result<u64> {
    let mut writer = WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    writer_fn(&mut writer)?;
    let bytes = writer.into_inner()?;

    let mut copy = conn.copy_in_raw(sql).await?;
    copy.send(bytes.as_slice()).await?;
    let rows = copy.finish().await?;
    Ok(rows)
}

async fn import_nutrients(conn: &mut PgConnection, path: &Path, ctx: &RowCtx) -> Result<()> {
    let sql = "COPY nutrition.nutrients(id, name, unit_name, nutrient_nbr, rank) \
               FROM STDIN WITH (FORMAT csv, NULL '')";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, ctx, |r, header| {
            let id = field_i32(r, header, "id")?;
            let name = field_str(r, header, "name")?;
            let unit = field_str(r, header, "unit_name")?;
            let nbr = field_opt(r, header, "nutrient_nbr").unwrap_or("");
            let rank = field_opt_i32(r, header, "rank");
            w.write_record([
                &id.to_string(),
                name,
                unit,
                nbr,
                &rank.map(|v| v.to_string()).unwrap_or_default(),
            ])?;
            Ok(())
        })
    })
    .await?;
    tracing::info!(rows, "imported nutrients");
    Ok(())
}

async fn import_measure_units(conn: &mut PgConnection, path: &Path, ctx: &RowCtx) -> Result<()> {
    if !path.exists() {
        tracing::warn!(?path, "measure_unit.csv missing, skipping");
        return Ok(());
    }
    let sql = "COPY nutrition.measure_units(id, name) FROM STDIN WITH (FORMAT csv, NULL '')";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, ctx, |r, header| {
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

async fn import_foods(
    conn: &mut PgConnection,
    path: &Path,
    data_types: &[String],
    ctx: &RowCtx,
) -> Result<HashSet<i32>> {
    let mut kept = HashSet::new();
    let sql = "COPY nutrition.foods(fdc_id, name, data_type, food_category_id, publication_date) \
               FROM STDIN WITH (FORMAT csv, NULL '')";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, ctx, |r, header| {
            let data_type = field_str(r, header, "data_type")?;
            if !data_types.iter().any(|d| d == data_type) {
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
    ctx: &RowCtx,
) -> Result<()> {
    let sql = "COPY nutrition.food_nutrients(id, fdc_id, nutrient_id, amount) \
               FROM STDIN WITH (FORMAT csv, NULL '')";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, ctx, |r, header| {
            let fdc_id = field_i32(r, header, "fdc_id")?;
            if !fdc_ids.contains(&fdc_id) {
                return Ok(());
            }
            let id = field_i64(r, header, "id")?;
            let nutrient_id = field_i32(r, header, "nutrient_id")?;
            let amount = field_opt_f64(r, header, "amount").unwrap_or(0.0);
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
    ctx: &RowCtx,
) -> Result<()> {
    if !path.exists() {
        tracing::warn!(?path, "food_portion.csv missing, skipping");
        return Ok(());
    }
    let sql = "COPY nutrition.food_portions(id, fdc_id, amount, measure_unit_id, \
               portion_description, modifier, gram_weight) \
               FROM STDIN WITH (FORMAT csv, NULL '')";
    let rows = copy_csv(conn, sql, |w| {
        each_record(path, ctx, |r, header| {
            let fdc_id = field_i32(r, header, "fdc_id")?;
            if !fdc_ids.contains(&fdc_id) {
                return Ok(());
            }
            let id = field_i64(r, header, "id")?;
            let amount = field_opt_f64(r, header, "amount");
            let measure_unit_id = field_opt_i32(r, header, "measure_unit_id");
            // empty portion_description / modifier go in as NULL thanks to
            // NULL '' on the COPY — lookup.rs treats NULL as "unresolved".
            let portion = field_opt(r, header, "portion_description").unwrap_or("");
            let modifier = field_opt(r, header, "modifier").unwrap_or("");
            let gram_weight = field_opt_f64(r, header, "gram_weight").unwrap_or(0.0);
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
    ctx: &RowCtx,
    mut per_row: impl FnMut(&StringRecord, &StringRecord) -> Result<()>,
) -> Result<()> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut rdr = ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(file);
    let header = rdr.headers()?.clone();
    let mut record = StringRecord::new();
    let mut row_num: u64 = 0;
    let mut skipped: u64 = 0;
    while rdr.read_record(&mut record)? {
        row_num += 1;
        if let Err(e) = per_row(&record, &header) {
            if ctx.continue_on_error {
                skipped += 1;
                tracing::warn!(
                    file = %path.display(),
                    row = row_num,
                    error = %e,
                    "skipping bad row"
                );
            } else {
                return Err(e.context(format!("{} row {}", path.display(), row_num)));
            }
        }
    }
    if skipped > 0 {
        tracing::warn!(file = %path.display(), skipped, total = row_num, "finished with skipped rows");
    }
    Ok(())
}

fn column_index(header: &StringRecord, name: &str) -> Result<usize> {
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
    field_str(r, header, name)?
        .parse()
        .with_context(|| format!("parse {name}"))
}

fn field_i64(r: &StringRecord, header: &StringRecord, name: &str) -> Result<i64> {
    field_str(r, header, name)?
        .parse()
        .with_context(|| format!("parse {name}"))
}

fn field_opt_i32(r: &StringRecord, header: &StringRecord, name: &str) -> Option<i32> {
    field_opt(r, header, name).and_then(|v| v.parse().ok())
}

fn field_opt_f64(r: &StringRecord, header: &StringRecord, name: &str) -> Option<f64> {
    field_opt(r, header, name).and_then(|v| v.parse().ok())
}
