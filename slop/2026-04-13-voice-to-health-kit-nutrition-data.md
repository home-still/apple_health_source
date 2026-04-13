# Voice meal logging for HealthKit: a complete technical blueprint

**A voice-described meal logger that writes all 38 HealthKit nutrients is fully achievable with on-device speech recognition, a cloud LLM for parsing, and USDA's nutrition database in Postgres — but the architecture requires several non-obvious design decisions.** Ollama Cloud now exists as a product but lacks the reliability needed for production mobile apps; Groq Cloud with Llama 3 is the strongest alternative. The USDA FoodData Central database covers 33 of 38 HealthKit nutrients well, with 5 trace nutrients (chromium, iodine, molybdenum, biotin, and partially caffeine) having sparse data. The most critical architectural choice is routing all LLM and database calls through the existing Rust Axum API server rather than calling from iOS directly, and implementing `HKCorrelation` with `HKMetadataKeySyncIdentifier` to prevent duplicate entries when the app's existing HealthKit read sync encounters its own written data.

---

## On-device speech recognition handles food descriptions well

Apple's `SFSpeechRecognizer` on iOS 17/18 provides solid on-device recognition for the short utterances typical of meal logging. Setting `requiresOnDeviceRecognition = true` removes the **1-minute audio duration limit** that applies to server-side recognition, eliminates the 1,000-request-per-hour device throttle, and keeps all audio data on-device — a meaningful privacy advantage for a health app.

The most powerful feature for this use case arrived in iOS 17: **custom language models via `SFCustomLanguageModelData`**. This API lets you train the recognizer on food-specific phrases ("quinoa bowl", "açaí smoothie", "chicken tikka masala") with frequency weighting and custom X-SAMPA pronunciations. Custom language models run strictly on-device and dramatically improve recognition of ethnic, specialty, and uncommon food names that the general Siri model handles poorly. The older `contextualStrings` property (limited to ~100 phrases) provides a simpler but less reliable alternative.

The modern Swift pattern wraps the callback-based API in `AsyncThrowingStream` for structured concurrency:

```swift
let request = SFSpeechAudioBufferRecognitionRequest()
request.requiresOnDeviceRecognition = true
request.addsPunctuation = true  // iOS 16+
request.customizedLanguageModel = foodLMConfig  // iOS 17+
request.taskHint = .dictation

func transcribe() -> AsyncThrowingStream<String, Error> {
    AsyncThrowingStream { continuation in
        recognitionTask = recognizer?.recognitionTask(with: request) { result, error in
            if let error { continuation.finish(throwing: error); return }
            if let result {
                continuation.yield(result.bestTranscription.formattedString)
                if result.isFinal { continuation.finish() }
            }
        }
    }
}
```

Under Swift 6 strict concurrency, `SFSpeechRecognizer` is not `Sendable`. The pragmatic approach is `@preconcurrency import Speech` with an actor-isolated speech service class. One known iOS 17 regression: `isAvailable` returns `true` even when Siri and Dictation are disabled in Settings, causing recognition tasks to fail silently — handle this with a graceful fallback.

**Could keyboard dictation suffice instead?** For an MVP, yes — a `UITextField` with the keyboard mic button requires zero code and produces usable text. But you lose programmatic control over start/stop, cannot boost food vocabulary, get no confidence scores or partial results, and the keyboard chrome feels heavy for a quick voice-logging interaction. For a polished product, `SFSpeechRecognizer` with a custom language model is clearly superior.

---

## Ollama Cloud exists but isn't ready for production mobile apps

**Ollama Cloud launched in preview around September 2025** and is live at ollama.com/cloud. It extends the local Ollama experience to cloud-hosted GPUs with the same API, CLI, and SDK — cloud models appear with a `-cloud` suffix and support structured outputs, tool calling, streaming, and the OpenAI-compatible endpoint. Plans are subscription-based: Free ($0), Pro ($20/month, 3 concurrent models), and Max ($100/month, 10 concurrent models).

However, Ollama Cloud is **not suitable for a production-facing mobile feature** for several reasons. There is **no SLA** — their terms explicitly disclaim uptime or latency guarantees. Performance benchmarks show ~95 tokens/sec for large models, roughly **2–13× slower than GPU-optimized competitors**. Cold starts of 10–15 seconds for less popular models would create unacceptable latency for interactive meal logging. Usage limits are described qualitatively ("light", "day-to-day", "heavy") with no published numeric quotas, and per-token pricing is listed as "coming soon." It's best suited for development and testing.

**Groq Cloud is the strongest production alternative.** Their custom LPU hardware delivers **840 tokens/sec on Llama 3.1 8B** — fast enough that meal parsing completes in under a second. Pricing is extraordinarily low: **$0.05 per million input tokens** for Llama 3.1 8B, meaning thousands of meal parses cost pennies. Critically, Groq supports **strict JSON Schema constrained decoding**, guaranteeing schema-compliant structured output — essential for reliable meal parsing. The API is fully OpenAI-compatible, making integration straightforward from a Rust backend using any OpenAI client library.

| Provider | Speed (Llama 3.1 8B) | Input cost/M tokens | Structured output | SLA |
|---|---|---|---|---|
| **Groq Cloud** | 840 TPS | $0.05 | Strict JSON Schema | Available |
| **Fireworks AI** | Fast | $0.10 | Strong support | 99.99% uptime |
| **Together AI** | Good | Competitive | JSON mode | 99% (Scale plan) |
| **OpenRouter** | Varies by backend | Pass-through + 5.5% | Depends on model | Varies |
| **Ollama Cloud** | ~95 TPS | Subscription only | Supported | None |

For meal parsing specifically, **Llama 3.1 8B on Groq is likely sufficient** — this is not a complex reasoning task, and the 8B model's speed and cost make it ideal for real-time interactive use.

---

## USDA FoodData Central covers 33 of 38 HealthKit nutrients comprehensively

The USDA FoodData Central database is the clear primary data source. The full CSV download is **458 MB zipped, 3.1 GB unzipped**, containing **400,000+ food items** across five dataset types. For a consumer meal-logging app, the optimal combination is **SR Legacy (~7,793 foods with up to 117 nutrients each) plus Foundation Foods (~2,100 foods with research-grade analytical data)** as the core, supplemented by **Branded Foods (~350,000 packaged products)** for label-scanned items.

The critical nutrient coverage analysis against HealthKit's 38 dietary types reveals **5 problematic nutrients**:

- **Chromium** — Analytical data essentially absent from core USDA datasets; only appears in branded/fortified products
- **Iodine** — Not in Foundation Foods or SR Legacy; USDA publishes a separate "Iodine Content of Common Foods" database that can supplement
- **Molybdenum** — Listed in Foundation Foods but limited coverage compared to major minerals
- **Biotin** — Not routinely analyzed for whole foods; available only for fortified/branded products
- **Caffeine** — Well-covered for beverages, coffee, tea, and chocolate (the foods where it matters), but zero for most other foods (expected)

The remaining **33 nutrients have good to excellent coverage** across macros, common minerals, and all vitamins. The honest approach for the 4 truly sparse nutrients is to display "data not available" when values are missing — this is standard practice across nutrition apps including Cronometer and MyFitnessPal.

**Importing into Postgres 17** is straightforward. The CSV download contains ~34 relational files (`food.csv`, `nutrient.csv`, `food_nutrient.csv`, `food_portion.csv`, `branded_food.csv`). Import via `\COPY` commands, then create GIN indexes for full-text search:

```sql
CREATE EXTENSION IF NOT EXISTS pg_trgm;

CREATE TABLE foods (
    id SERIAL PRIMARY KEY,
    fdc_id INTEGER UNIQUE,
    name TEXT NOT NULL,
    data_source VARCHAR(20),
    serving_size_g REAL,
    search_vector tsvector 
        GENERATED ALWAYS AS (to_tsvector('english', name)) STORED
);
CREATE INDEX idx_foods_search ON foods USING gin(search_vector);
CREATE INDEX idx_foods_trgm ON foods USING gin(name gin_trgm_ops);

CREATE TABLE food_nutrients (
    food_id INTEGER REFERENCES foods(id),
    nutrient_id INTEGER REFERENCES nutrients(id),
    amount_per_100g REAL NOT NULL,
    UNIQUE(food_id, nutrient_id)
);
```

**Database size is very manageable**: SR Legacy + Foundation Foods produces ~600K food_nutrient rows in ~100–150 MB. Adding Branded Foods pushes to ~2–3 GB. With indexes, the entire dataset fits comfortably on a **4 GB VPS** — well within the existing Postgres 17 instance capacity.

**Open Food Facts** (4+ million products, ODbL license) serves as a useful supplement for international branded products and barcode lookup, but its nutrient depth is shallow — typically only 8–15 nutrients per product versus USDA's 50–117. Use it for product identification, not nutrition values.

---

## Writing nutrition data to HealthKit requires correlations and careful metadata

HealthKit exposes **39 dietary quantity type identifiers** (the 38 listed plus `.dietaryChloride`). Each food entry should be written as an `HKCorrelation` of type `.food` that groups all nutrient `HKQuantitySample` objects from a single food item. While correlations aren't strictly required, they're the established best practice — MyFitnessPal, Lose It!, and Cronometer all use this pattern, and the Health app displays correlated food entries as grouped items.

The key metadata keys for food logging:

- **`HKMetadataKeyFoodType`** (official) — The food name string displayed in Health app (e.g., "Grilled Chicken Breast")
- **`"HKFoodMeal"`** (unofficial but widely used) — Meal type string ("Breakfast", "Lunch", "Dinner", "Snack")
- **`HKMetadataKeySyncIdentifier`** — Critical for deduplication; saving an object with the same sync identifier and higher `HKMetadataKeySyncVersion` replaces the old entry
- **`HKMetadataKeyWasUserEntered`** — Mark as `true` since this is user-initiated data

Only write non-zero nutrient values — skip creating samples where the amount is 0. This reduces HealthKit database bloat and is the pattern used by all major food logging apps. The complete save operation wraps all samples into a single `HKCorrelation` and calls `healthStore.save()` once, which atomically persists the correlation and all contained samples.

Authorization requires both `NSHealthShareUsageDescription` and `NSHealthUpdateUsageDescription` in Info.plist, plus the HealthKit capability in Xcode. The authorization sheet presents **separate toggles for each nutrient type** — users can deny individual nutrients. Handle `HKError.errorAuthorizationDenied` gracefully and guide users to Settings > Health if needed.

---

## The LLM should parse food items, not estimate nutrition values

Research from the **NutriBench benchmark** (UC Santa Barbara) — testing 11,857 meal descriptions — demonstrates that **~80% of LLM nutrition errors come from incorrect nutritional knowledge or hallucinations**, not from food identification failures. The clear conclusion: use the LLM exclusively for natural language parsing, and use the verified USDA database for actual nutrient values.

The optimal architecture is a **two-stage pipeline**:

**Stage 1 — LLM parses meal description into structured food items.** The LLM receives natural language ("I had about 6oz of grilled chicken breast and a cup of brown rice") and outputs structured JSON with food names, quantities, units, preparation methods, confidence levels, and database search terms. **Chain-of-Thought prompting** is the single most effective technique — NutriBench found it "significantly mitigates error" for multi-item meals. Use strict JSON Schema mode (available on Groq) for guaranteed schema compliance.

**Stage 2 — Database lookup retrieves verified nutrition data.** For each parsed food item, query the Postgres nutrition database using the LLM-generated search terms. PostgreSQL's `ts_rank` with `pg_trgm` fuzzy matching handles synonym resolution ("chicken breast" → "Chicken, broilers or fryers, breast, skinless, boneless, meat only, cooked, grilled"). Convert the user's quantity to the database's per-100g reference amount using deterministic code, not the LLM — serving unit conversion is the most common error category in LLM nutrition estimation.

The JSON schema for Stage 1 should include `food_name`, `quantity`, `unit`, `preparation_method`, `confidence` (high/medium/low), and `database_search_terms` (an ordered array of progressively simpler search terms for database matching). For vague quantities, the LLM maps to reasonable defaults: "some" → 0.5 serving, "a handful" → 1 oz for nuts, "a bowl" → 1.5–2 cups. Missing quantities default to 1 standard USDA serving with medium confidence. Compound foods like "caesar salad" get decomposed into constituent ingredients with estimated proportions.

---

## Architecture integration with the existing HealthSync stack

Given the existing architecture — a Swift iOS app reading from HealthKit and syncing to a Rust Axum API backed by Postgres 17 with TimescaleDB — the meal logging feature introduces the app's first HealthKit **write** path. This has significant implications for the sync protocol.

**LLM and database calls should route through the Rust API server, not from iOS directly.** This keeps the LLM API key server-side (never embedded in the iOS binary), enables server-side rate limiting and request logging, allows the Rust server to orchestrate the two-stage pipeline (LLM parse → Postgres lookup → response) in a single round-trip, and lets you cache common food lookups. The iOS app sends the transcribed text to a single `/api/meals/parse` endpoint and receives the complete structured nutrition response.

**The nutrition database lives alongside the existing Postgres 17 instance.** Since the USDA data for SR Legacy + Foundation Foods is only ~150 MB, it adds negligible load to the existing TimescaleDB setup. Create a separate `nutrition` schema to isolate it from the sync tables. The Rust server handles the food name matching and nutrient calculation, returning a `MealNutrition` struct that the iOS app writes to HealthKit.

**The sync protocol needs a deduplication strategy.** The current app reads HealthKit data and syncs it to the server. Once the app starts writing food entries to HealthKit, the read sync will encounter its own written data — creating a potential duplication loop. The solution is **`HKMetadataKeySyncIdentifier`**: tag every written food correlation with a unique identifier (e.g., `healthsync-meal-{uuid}`). During the read sync, filter out samples whose sync identifier matches the app's prefix. Additionally, store the meal's UUID on the server side when the parse request completes, so the server can deduplicate if the same meal arrives through both the write-then-read-sync path and any future direct API submission.

The complete request flow looks like this:

1. **iOS**: User taps mic → `SFSpeechRecognizer` (on-device, custom food LM) → transcribed text
2. **iOS → Rust API**: `POST /api/meals/parse` with `{ text: "6oz grilled chicken and a cup of brown rice", meal_type: "dinner" }`
3. **Rust API → Groq**: LLM call with system prompt + user text → structured JSON of parsed food items
4. **Rust API → Postgres**: For each food item, full-text search → retrieve nutrient values → scale by portion size
5. **Rust API → iOS**: Return complete `MealNutrition` response with all nutrient values
6. **iOS**: User reviews/confirms → write `HKCorrelation(.food)` with all nutrient samples to HealthKit
7. **iOS**: Store meal record locally with sync identifier to prevent read-sync duplication

**One additional consideration**: the Rust API should persist the raw meal log (original text, parsed items, matched foods, final nutrient values) in a `meal_logs` table. This provides an audit trail, enables reprocessing if the USDA data is updated, and supports future features like meal history and nutrition trends independent of HealthKit.

---

## Conclusion

This feature is architecturally clean because each component does what it's best at: the iPhone handles speech recognition with food-specific vocabulary boosting, the LLM excels at natural language parsing into structured data, Postgres delivers verified nutrition values through full-text search, and HealthKit provides the standardized health data layer. The two decisions most likely to cause regret if made wrong are the LLM provider choice (Groq's speed and structured output guarantees make it the clear winner over Ollama Cloud for production use) and the sync deduplication strategy (tag all written correlations with `HKMetadataKeySyncIdentifier` from day one, or face data duplication bugs that are painful to fix retroactively). The 5 sparse nutrients (chromium, iodine, molybdenum, biotin) are a known limitation of every nutrition database — display "not available" honestly rather than estimating, and consider supplementing with USDA's separate iodine database if iodine tracking matters to your users.