# Backlog — Voice Meal Logging → HealthKit

Source: `slop/2026-04-13-voice-to-health-kit-nutrition-data.md`
Plan: `~/.claude/plans/sleepy-enchanting-cat.md`

Decisions locked in v1:
- **LLM:** Ollama Cloud (OpenAI-compatible at `https://ollama.com/v1`, key `OLLAMA_API_KEY`, model `gpt-oss:120b-cloud` by default).
- **Nutrition:** USDA SR Legacy + Foundation Foods only (Branded / Iodine deferred).
- **Sync dedup:** every written food correlation tagged `HKMetadataKeySyncIdentifier = "healthsync-meal-<uuid>"`.

---

## M1 — USDA nutrition schema + import
- [ ] `api/migrations/20260413000001_create_nutrition_schema.sql` (pg_trgm, foods/nutrients/food_nutrients/food_portions, GIN indexes)
- [ ] `api/migrations/20260413000002_seed_nutrient_healthkit_map.sql` (USDA id → HK identifier, `sparse` flag)
- [ ] `api/src/bin/import_usda.rs` (downloads/\COPYs the 5 core CSVs)
- [ ] `api/Cargo.toml`: add `csv`, `reqwest`, `zip`
- [ ] Verify: `SELECT count(*) FROM nutrition.foods` ≈ 9,900

## M2 — Ollama Cloud LLM client
- [ ] `api/Cargo.toml`: `reqwest`, `async-trait`, `wiremock` (dev)
- [ ] `api/src/config.rs`: `OLLAMA_API_KEY`, `OLLAMA_BASE_URL`, `OLLAMA_MODEL`
- [ ] `api/src/llm/mod.rs`: `LlmClient` trait
- [ ] `api/src/llm/ollama.rs`: `OllamaClient` impl
- [ ] `api/src/llm/prompt.rs`: system prompt + JSON schema
- [ ] `api/src/models/meal.rs`: `ParsedMeal`, `MealParseRequest`, `MealNutritionResponse`
- [ ] Unit tests with `wiremock`

## M3 — `/api/meals/parse` + nutrition lookup
- [ ] `api/src/nutrition/{mod,lookup,units}.rs`
- [ ] `api/src/handlers/meals_handler.rs`
- [ ] `api/src/routes/meals_routes.rs`
- [ ] `api/src/routes/mod.rs`: register
- [ ] Integration test with stubbed `LlmClient`

## M4 — `meal_logs` audit
- [ ] `api/migrations/20260413000003_create_meal_logs.sql`
- [ ] Handler persists raw + parsed + matched + final nutrients

## M5 — iOS SpeechService
- [ ] `ios/project.yml`: `NSMicrophoneUsageDescription`, `NSSpeechRecognitionUsageDescription`
- [ ] `ios/HealthSync/Speech/SpeechService.swift` (actor, on-device recognition)
- [ ] `ios/HealthSync/Speech/FoodLanguageModel.swift` (stub)

## M6 — iOS HealthKit write path
- [ ] `ios/HealthSync/HealthKit/HKTypes.swift`: `allWriteTypes`
- [ ] `ios/HealthSync/HealthKit/HKManager.swift`: auth + `writeMealCorrelation`
- [ ] `ios/HealthSync/Models/MealNutrition.swift`

## M7 — MealLogView UI
- [ ] `ios/HealthSync/Sync/APIClient.swift`: `parseMeal`
- [ ] `ios/HealthSync/Views/MealLogView.swift`
- [ ] `ios/HealthSync/Views/DashboardView.swift`: nav entry

## M8 — Read-sync dedup
- [ ] `ios/HealthSync/Sync/SyncEngine.swift`: `isAppWritten` filter
- [ ] `ios/HealthSyncTests/HealthSyncTests.swift`: unit test

## M9 — Deferred
- [x] `SFCustomLanguageModelData` food vocabulary — 130-phrase corpus, cached compile (7c1242f)
- [x] Branded Foods import — `--data-types` flag opts it in (f212484)
- [x] USDA Iodine supplemental dataset — table + fallback lookup (09fa721)
- [x] Meal history view — GET /api/meals/history + iOS list (b3c0251)
- [x] API-side food-lookup cache — moka in-memory LRU (55b57d1)
