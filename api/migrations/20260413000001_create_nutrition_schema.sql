CREATE SCHEMA IF NOT EXISTS nutrition;

CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TABLE nutrition.nutrients (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    unit_name TEXT NOT NULL,
    nutrient_nbr TEXT,
    rank INTEGER
);

CREATE TABLE nutrition.foods (
    fdc_id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    data_type TEXT NOT NULL,
    food_category_id INTEGER,
    publication_date DATE,
    search_vector tsvector GENERATED ALWAYS AS (to_tsvector('english', name)) STORED
);
CREATE INDEX idx_foods_search_vector ON nutrition.foods USING gin(search_vector);
CREATE INDEX idx_foods_name_trgm ON nutrition.foods USING gin(name gin_trgm_ops);
CREATE INDEX idx_foods_data_type ON nutrition.foods(data_type);

CREATE TABLE nutrition.food_nutrients (
    id BIGINT PRIMARY KEY,
    fdc_id INTEGER NOT NULL REFERENCES nutrition.foods(fdc_id) ON DELETE CASCADE,
    nutrient_id INTEGER NOT NULL REFERENCES nutrition.nutrients(id) ON DELETE CASCADE,
    amount REAL NOT NULL
);
CREATE INDEX idx_food_nutrients_fdc ON nutrition.food_nutrients(fdc_id);
CREATE INDEX idx_food_nutrients_nutrient ON nutrition.food_nutrients(nutrient_id);

CREATE TABLE nutrition.measure_units (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE nutrition.food_portions (
    id BIGINT PRIMARY KEY,
    fdc_id INTEGER NOT NULL REFERENCES nutrition.foods(fdc_id) ON DELETE CASCADE,
    amount REAL,
    measure_unit_id INTEGER REFERENCES nutrition.measure_units(id),
    portion_description TEXT,
    modifier TEXT,
    gram_weight REAL NOT NULL
);
CREATE INDEX idx_food_portions_fdc ON nutrition.food_portions(fdc_id);
