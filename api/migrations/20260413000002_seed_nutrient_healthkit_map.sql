CREATE TABLE nutrition.nutrient_healthkit_map (
    nutrient_id INTEGER PRIMARY KEY,
    hk_identifier TEXT NOT NULL UNIQUE,
    sparse BOOLEAN NOT NULL DEFAULT FALSE
);

-- USDA nutrient IDs → HealthKit HKQuantityTypeIdentifier strings.
-- Covers the 38 HealthKit dietary types plus DietaryChloride (39 total).
-- `sparse = true` flags nutrients with poor coverage in USDA core datasets —
-- the handler returns null for these rather than a zero.
INSERT INTO nutrition.nutrient_healthkit_map (nutrient_id, hk_identifier, sparse) VALUES
    (1008, 'HKQuantityTypeIdentifierDietaryEnergyConsumed',      false),
    (1003, 'HKQuantityTypeIdentifierDietaryProtein',             false),
    (1004, 'HKQuantityTypeIdentifierDietaryFatTotal',            false),
    (1258, 'HKQuantityTypeIdentifierDietaryFatSaturated',        false),
    (1292, 'HKQuantityTypeIdentifierDietaryFatMonounsaturated',  false),
    (1293, 'HKQuantityTypeIdentifierDietaryFatPolyunsaturated',  false),
    (1253, 'HKQuantityTypeIdentifierDietaryCholesterol',         false),
    (1005, 'HKQuantityTypeIdentifierDietaryCarbohydrates',       false),
    (1079, 'HKQuantityTypeIdentifierDietaryFiber',               false),
    (2000, 'HKQuantityTypeIdentifierDietarySugar',               false),
    (1051, 'HKQuantityTypeIdentifierDietaryWater',               false),
    (1057, 'HKQuantityTypeIdentifierDietaryCaffeine',            true),
    (1093, 'HKQuantityTypeIdentifierDietarySodium',              false),
    (1092, 'HKQuantityTypeIdentifierDietaryPotassium',           false),
    (1087, 'HKQuantityTypeIdentifierDietaryCalcium',             false),
    (1089, 'HKQuantityTypeIdentifierDietaryIron',                false),
    (1090, 'HKQuantityTypeIdentifierDietaryMagnesium',           false),
    (1091, 'HKQuantityTypeIdentifierDietaryPhosphorus',          false),
    (1095, 'HKQuantityTypeIdentifierDietaryZinc',                false),
    (1098, 'HKQuantityTypeIdentifierDietaryCopper',              false),
    (1101, 'HKQuantityTypeIdentifierDietaryManganese',           false),
    (1103, 'HKQuantityTypeIdentifierDietarySelenium',            false),
    (1088, 'HKQuantityTypeIdentifierDietaryChloride',            false),
    (1096, 'HKQuantityTypeIdentifierDietaryChromium',            true),
    (1100, 'HKQuantityTypeIdentifierDietaryIodine',              true),
    (1102, 'HKQuantityTypeIdentifierDietaryMolybdenum',          true),
    (1106, 'HKQuantityTypeIdentifierDietaryVitaminA',            false),
    (1165, 'HKQuantityTypeIdentifierDietaryThiamin',             false),
    (1166, 'HKQuantityTypeIdentifierDietaryRiboflavin',          false),
    (1167, 'HKQuantityTypeIdentifierDietaryNiacin',              false),
    (1170, 'HKQuantityTypeIdentifierDietaryPantothenicAcid',     false),
    (1175, 'HKQuantityTypeIdentifierDietaryVitaminB6',           false),
    (1176, 'HKQuantityTypeIdentifierDietaryBiotin',              true),
    (1177, 'HKQuantityTypeIdentifierDietaryFolate',              false),
    (1178, 'HKQuantityTypeIdentifierDietaryVitaminB12',          false),
    (1162, 'HKQuantityTypeIdentifierDietaryVitaminC',            false),
    (1114, 'HKQuantityTypeIdentifierDietaryVitaminD',            false),
    (1109, 'HKQuantityTypeIdentifierDietaryVitaminE',            false),
    (1185, 'HKQuantityTypeIdentifierDietaryVitaminK',            false);
