# Commit Review Log — Voice Meal Logging
**Range:** `8e7005e..42cf0ad` (7 feature commits)
**Reviewed:** 2026-04-13
**Reviewers:** 5 parallel Explore agents (A: endpoint, B: nutrition, C: LLM, D: HK+speech, E: iOS UI)

## Summary

| Severity | Count |
|----------|-------|
| Critical | 5 |
| High     | 9 |
| Medium   | 14 |
| Low      | 9 |

**Top 3 items to fix first:**

1. **PII / secret leakage in LLM error logs** — `api/src/llm/ollama.rs:89` + `:109` include the raw Ollama response body (and therefore the user's meal text, possibly dietary/medical info) inside `AppError::Internal`, which `error.rs:27` then writes via `tracing::error!`. [CRIT-3]
2. **No reqwest timeout on Ollama client** — `api/src/llm/ollama.rs:21` uses `Client::new()` (infinite timeout). Cold starts of 10–15s and outages can pin request-handler tokio tasks indefinitely. [CRIT-4]
3. **Partial HealthKit authorization silently succeeds** — `HKManager.requestAuthorization` (`HKManager.swift:23`) sets `isAuthorized = true` as long as the sheet didn't throw, even if the user denied individual dietary types. Every subsequent `writeMealCorrelation` silently drops denied nutrients. [CRIT-2]

---

## Findings

### [CRITICAL] `api/src/nutrition/lookup.rs:76` — Iodine fuzzy match has no similarity floor
**Agent:** B
**Issue:** `iodine_supplemental()` picks the top row ordered by `ts_rank` then `similarity`, but never enforces a minimum score. For a term with no real match, Postgres still returns the best-of-nothing — e.g. "cod salad" could inherit yogurt's iodine value.
**Suggestion:** Add `WHERE (search_vector @@ plainto_tsquery('english', $1) AND ts_rank(...) > 0.05) OR name % $1 AND similarity(name, $1) > 0.3`.

### [CRITICAL] `ios/HealthSync/HealthKit/HKManager.swift:23` — Partial dietary-write denial undetected
**Agent:** D
**Issue:** `requestAuthorization(toShare: allWriteTypes, ...)` returns success regardless of which of the 39 dietary types the user actually permitted. `isAuthorized = true` hides the fact, and `writeMealCorrelation` later silently skips denied types.
**Suggestion:** After auth, iterate `allWriteTypes` and `authorizationStatus(for:)` each; store the authorized subset and filter `writeMealCorrelation` samples against it; surface missing nutrients in UI.

### [CRITICAL] `api/src/llm/ollama.rs:89, 109` — PII leakage via LLM error body in logs
**Agent:** C
**Issue:** Both the HTTP-error path and the JSON-schema-violation path embed `resp.text()` / `content` directly into `AppError::Internal(...)`. That message flows to `error.rs:27` which does `tracing::error!`, landing the user's meal description (and any Ollama-echoed request context) in logs + external monitoring.
**Suggestion:** Truncate to first ~200 chars for logging, strip from the user-facing error; return a generic `"external service error"` to the client. Log at `trace!`/`debug!`, never `error!`.

### [CRITICAL] `api/src/llm/ollama.rs:21` — No reqwest timeout
**Agent:** C
**Issue:** `Client::new()` uses infinite defaults. Ollama Cloud documents 10–15s cold starts and has no SLA. A hung response pins the handler's tokio task forever, starving concurrency.
**Suggestion:** `Client::builder().timeout(Duration::from_secs(45)).build()?`; optionally a per-request deadline.

### [CRITICAL] `ios/HealthSync/HealthKit/HKManager.swift:65` — `HKMetadataKeySyncVersion` hardcoded to 1
**Agent:** D
**Issue:** The slop doc specifies that saving the same `sync_identifier` with a *higher* `SyncVersion` replaces the prior entry. We always write `1`, so that semantic is unreachable. Today every meal has a fresh UUID so this is latent, but it blocks meal edits.
**Suggestion:** Track a per-identifier version in local state (e.g., user defaults keyed by sync identifier), increment on re-save.

---

### [HIGH] `api/migrations/20260413000003_create_meal_logs.sql:7` — `sync_identifier UNIQUE` + fresh UUID per request → duplicate rows on retry
**Agent:** A
**Issue:** `parse_meal` generates `Uuid::new_v4()` per request, so client retries after a transient failure get a brand-new identifier and succeed — duplicating the logical meal on the server.
**Suggestion:** Accept an optional client-supplied `sync_identifier` (or an `Idempotency-Key` header), check for a prior row before running the LLM, and use that UUID in both `meal_logs` and the HK correlation.

### [HIGH] `api/src/main.rs:40` — `CorsLayer::permissive()` in production
**Agent:** A
**Issue:** Allows any origin + credentials — CSRF surface for any authenticated user browsing a malicious page.
**Suggestion:** Restrict with `CorsLayer::new().allow_origin(...).allow_methods(...).allow_headers(...)` scoped to the iOS bundle's expected origins (or drop CORS entirely since the client is native).

### [HIGH] `api/src/handlers/meals_handler.rs:~32-70` — LLM call outside any transaction; retries re-bill
**Agent:** A
**Issue:** LLM call happens, then separate `INSERT`. If the insert fails (uniqueness, DB blip), the LLM cost is sunk; the client retry re-runs the LLM.
**Suggestion:** Pair with the idempotency fix above: look up existing row by idempotency key *before* the LLM call; wrap INSERT + side effects in a transaction.

### [HIGH] `ios/HealthSync/Speech/SpeechService.swift:57-66` — Audio tap leaks on `audioEngine.start()` failure
**Agent:** D
**Issue:** Tap is installed before `start()`. If `start()` throws, the tap remains on `inputNode`; subsequent `transcribe()` calls install a second tap (`installTap` is permissive but `stop()` won't remove the orphaned one because `isRunning == false`).
**Suggestion:** `defer` a `removeTap` on the error path, or install the tap *after* `start()`.

### [HIGH] `ios/HealthSync/Speech/SpeechService.swift:44` — First-launch LM compile blocks the recorder
**Agent:** D
**Issue:** `FoodLanguageModel.configuration()` synchronously exports + compiles the corpus inside `transcribe()`; can take seconds, during which the UI shows nothing.
**Suggestion:** Kick off `compileIfNeeded` in the app-launch task (or on `MealLogView.task`), fall back to `nil` configuration if still compiling so recording isn't blocked.

### [HIGH] `ios/HealthSync/Sync/SyncEngine.swift:~75-80` — Prefix string duplicated
**Agent:** D
**Issue:** `isAppWritten` uses literal `"healthsync-meal-"` but HKManager defines `mealSyncIdentifierPrefix` — the static function references the typed constant, which is fine today, but the prefix is also stored in HK metadata where it was *written* as a formatted string; any drift between writer and filter breaks dedup.
**Suggestion:** Keep the constant single-sourced (already correct); add a unit test asserting writer-side formatting uses the same constant (`"\(mealSyncIdentifierPrefix)\(uuid)"`).

### [HIGH] `api/src/bin/import_usda.rs:73` — Importer PK collisions without `--reset`
**Agent:** B
**Issue:** Re-running without `--reset` into `foods`/`food_nutrients` (PK on `fdc_id`, `id`) makes `COPY` fail partway with a generic error; no row-level diagnostic.
**Suggestion:** Either pre-check row counts and bail with a message, or switch to `COPY ... INTO temp` + `INSERT ... ON CONFLICT DO UPDATE`.

### [HIGH] `api/src/bin/import_usda.rs:235-246` — Empty portion/modifier stored as `""` (not NULL)
**Agent:** B
**Issue:** `field_opt(...).unwrap_or("")` writes empty strings; `match_portion` then treats `unit == "" || desc == ""` as "matches everything" since `"".contains("slice") == false` but `"".contains("") == true`. Any caller passing an empty unit would get an incorrect gram weight.
**Suggestion:** Write `NULL` when the CSV cell is empty; treat empty `unit` as invalid in `match_portion`.

### [HIGH] `api/src/nutrition/lookup.rs:~96` — `scale_nutrients(per_100g, 0.0)` yields silent zeros
**Agent:** B
**Issue:** When `to_grams` returns `Some(0.0)` (portion with `gram_weight = 0`) or `None` (unknown unit → handler passes empty list, not 0), the user gets an empty/zero response with no explanation.
**Suggestion:** In `resolve_item`, treat `grams.is_none()` or `<= 0` as a parse warning attached to `MatchedItem`; keep returning the food match but label nutrients as unknown.

### [HIGH] `api/src/llm/ollama.rs:67-73` — Strict-mode enforcement unverified; no schema-violation test
**Agent:** C
**Issue:** We assume Ollama Cloud honors `strict: true`. The single test covers only the happy path, so a schema violation would bubble as 500.
**Suggestion:** Add wiremock tests for 401/429/5xx, empty `choices`, malformed JSON, and schema-violating JSON. On schema violation, return `AppError::BadRequest("meal parse failed")` so the client shows a sensible message.

---

### [MEDIUM] `api/src/handlers/meals_handler.rs:~28` — Unbounded `text` / unvalidated `meal_type`
**Agent:** A
**Issue:** Only emptiness is checked. Multi-MB bodies fit under the 50 MB global limit, wasting LLM tokens; `meal_type` accepts any string.
**Suggestion:** `if payload.text.len() > 2_000 { return BadRequest }`; validate `meal_type` against `["breakfast","lunch","dinner","snack"]` (case-insensitive).

### [MEDIUM] `api/src/main.rs:38` — `DefaultBodyLimit::max(50 MB)` is global
**Agent:** A
**Issue:** 50 MB is fine for sync payloads but too permissive for the meal endpoint.
**Suggestion:** Scope per-route: `.route("/api/meals/parse", post(...).layer(DefaultBodyLimit::max(64_000)))`.

### [MEDIUM] `api/src/nutrition/cache.rs` — No invalidation after re-import
**Agent:** B
**Issue:** 24 h TTL means stale `fdc_id` entries for up to a day after `import_usda --reset`, producing empty nutrient rows without error.
**Suggestion:** Either bump the cache version on import (simplest: `cache.invalidate_all()` via an admin endpoint or a file sentinel), or key entries by `fdc_id + dataset_version`.

### [MEDIUM] `ios/HealthSync/Models/MealNutrition.swift:~135` — Micro-glyph unit handling is accidental
**Agent:** E
**Issue:** USDA emits "µg" sometimes; `raw.uppercased()` turns µ (U+00B5) into Μ (U+039C), which happens to appear in the switch. Any refactor that drops that line silently loses micrograms.
**Suggestion:** Normalize explicitly: `raw.replacingOccurrences(of: "µ", with: "u").uppercased()` before the switch, and cover this case in a unit test.

### [MEDIUM] `ios/HealthSync/Sync/APIClient.swift:~158` — `URL(string: "/api/...", relativeTo: baseURL)` ignores baseURL path
**Agent:** E
**Issue:** If a future baseURL has a path prefix (`https://host/v2`), the absolute path `/api/meals/history?...` replaces it.
**Suggestion:** Drop the leading slash or use `baseURL.appendingPathComponent("api/meals/history")` + `URLComponents` for the query.

### [MEDIUM] `ios/HealthSync/Views/MealLogView.swift:~152` — Save doesn't pre-check HK write auth
**Agent:** E
**Issue:** Denial surfaces as a runtime throw with an opaque message.
**Suggestion:** Disable the Save button or present a "Grant HealthKit write access" CTA before calling `writeMealCorrelation`.

### [MEDIUM] `ios/HealthSync/HealthKit/HKManager.swift:47` — Instant samples (`start == end`) for meals
**Agent:** D
**Issue:** HealthKit semantics treat `start == end` as instantaneous; most food-log apps set a small span (e.g., 15-30 min window centered on the eat time) for aggregation/graphing.
**Suggestion:** Accept `mealStart`/`mealEnd` params; default to `start = now, end = now` but allow a short span for future UX.

### [MEDIUM] `api/src/llm/prompt.rs:3-19` — Decomposition instruction may outpace USDA coverage
**Agent:** C
**Issue:** Prompt tells the LLM to decompose compound foods unless present in USDA, which the model doesn't actually know.
**Suggestion:** Simplify — always surface the user's stated food; let the nutrition layer fall back to fuzzy match / component search.

### [MEDIUM] `api/src/llm/ollama.rs:~100` — No retry / rate-limit backoff
**Agent:** C
**Issue:** 429/5xx = immediate 500; any Ollama hiccup surfaces to users.
**Suggestion:** 3× exponential backoff on 429/5xx, jittered.

### [MEDIUM] `ios/HealthSync/Speech/SpeechService.swift:36` — iOS 17 `isAvailable` regression unhandled
**Agent:** D
**Issue:** `isAvailable` can return `true` while Siri is disabled; the task then hangs silently.
**Suggestion:** Wrap the stream in a 10s no-result watchdog and surface `.recognizerFailure("speech service silent — check Siri/Dictation settings")`.

### [MEDIUM] `api/src/bin/import_usda.rs:63-68` — Empty `--data-types ""` silently imports zero foods
**Agent:** B
**Issue:** Filter is applied; no warning when parsed list is empty.
**Suggestion:** Bail with `anyhow::bail!("--data-types must include at least one data_type")`.

### [MEDIUM] `api/src/handlers/meals_handler.rs:~113` — `None` grams returns empty nutrient array without warning
**Agent:** B
**Issue:** Unrecognized unit → `nutrients: []`, client has no idea.
**Suggestion:** Attach a per-item warning flag (`unit_unresolved: true`) in `MatchedItem`.

### [MEDIUM] `api/src/bin/import_usda.rs:~255` — Malformed CSV row halts with no row number
**Agent:** B
**Issue:** `field_i32` error propagates upward; operator sees `parse fdc_id` with no location.
**Suggestion:** Add a row counter; log `row_num=N, field=fdc_id, value="...", err=...` and optionally a `--continue-on-error` flag.

### [MEDIUM] `ios/HealthSync/Info.plist` / `project.yml` — `NSAllowsArbitraryLoads: true` in production build
**Agent:** E
**Issue:** Pre-existing, but worth gating. Disables ATS app-wide.
**Suggestion:** Keep ATS on; add specific exceptions for localhost only in a debug `.xcconfig`.

---

### [LOW] `api/src/llm/ollama.rs:~18` — API key not validated at startup
**Agent:** C
**Suggestion:** Non-empty check in `OllamaClient::new`; optional health probe.

### [LOW] `api/src/models/meal.rs:16` — `preparation_method` has no validation
**Agent:** C
**Suggestion:** Free-form is fine today; document expectation.

### [LOW] `api/src/llm/ollama.rs` — Only happy-path test coverage
**Agent:** C
**Suggestion:** See HIGH test-gap item above.

### [LOW] `ios/HealthSync/HealthKit/HKTypes.swift:256-296` — 39 hard-coded dietary identifiers
**Agent:** D
**Suggestion:** OK for now; if server `nutrient_healthkit_map` grows, consider generating the Swift list from the server map at build time.

### [LOW] `ios/HealthSync/Speech/SpeechService.swift` — `@preconcurrency import Speech`
**Agent:** D
**Suggestion:** Actor isolation is correct; add a doc comment clarifying the boundary.

### [LOW] `ios/HealthSync/Views/MealLogView.swift:~156` — `compactMap` used where `map` suffices
**Agent:** E
**Suggestion:** `map { $0.matchedFood?.name ?? $0.parsed.foodName }`.

### [LOW] `ios/HealthSync/Views/MealLogView.swift` — No loading spinners during parse/save
**Agent:** E
**Suggestion:** `ProgressView` overlay while `isParsing || isSaving`.

### [LOW] `ios/HealthSync/Views/MealLogView.swift` — No parse cancellation, no URLSession timeout
**Agent:** E
**Suggestion:** Wrap the `APIClient.parseMeal` call in `Task` you hold; configure a 30s request timeout.

### [LOW] `api/src/nutrition/units.rs:24-26` — Implicit 0-amount fallback to 1.0
**Agent:** B
**Suggestion:** Comment the intent; optionally validate amounts at import.

---

## Raw agent reports

### Agent A — Backend /api/meals endpoint
- [CRIT] Error body exposure (ollama.rs — but note: Agent A mapped this onto the endpoint, Agent C owns it; deduplicated above).
- [HIGH] sync_identifier UNIQUE + retries → duplicates (covered above).
- [HIGH] CorsLayer::permissive.
- [HIGH] LLM call outside transaction.
- [MED] Unbounded text/meal_type.
- [MED] 50 MB body limit global.
- [MED] ollama.rs:109 JSON body in error.
- [LOW] NutritionCache Send/Sync — no bug.
- [LOW] user_id vs claims.sub pattern check — correct today.

### Agent B — Nutrition data layer
- [CRIT] Iodine fuzzy match threshold.
- [HIGH] Empty portion fields → bad grams.
- [HIGH] scale_nutrients zero cascade.
- [HIGH] Importer PK collisions without --reset.
- [MED] Empty --data-types silent no-op.
- [MED] Moka cache staleness post-import.
- [MED] nutrient_healthkit_map has no FK (intentional).
- [MED] None grams silent empty output.
- [MED] CSV row-level errors lack context.
- [LOW] units.rs 0-amount fallback.
- [LOW] Triple-bind parameter note (no bug).

### Agent C — LLM client
- [CRIT] PII leak in error body (ollama.rs:109).
- [CRIT] No reqwest timeout.
- [HIGH] Unverified strict mode; no schema-violation tests.
- [HIGH] HTTP response body not redacted (ollama.rs:89).
- [MED] No API key validation at instantiation.
- [MED] No retries / backoff on 429/5xx.
- [MED] Prompt decomposition logic may mismatch USDA.
- [LOW] Test coverage: happy-path only.
- [LOW] preparation_method unvalidated.

### Agent D — iOS HealthKit + speech
- [CRIT] SyncVersion always = 1; replace-on-version unreachable.
- [CRIT] Partial dietary-type auth silently succeeds.
- [HIGH] Audio tap leaks on `audioEngine.start()` failure.
- [HIGH] FoodLanguageModel compile blocks first recording.
- [HIGH] `isAppWritten` prefix duplication (shared constant exists but fragile).
- [MED] Instant samples (start == end) for meals.
- [MED] iOS 17 `isAvailable` regression unhandled.
- [MED] onTermination double-`stop()` benign but non-idiomatic.
- [MED] AVAudioSession contention not distinguished.
- [LOW] 39-type hardcoded list.
- [LOW] `@preconcurrency` boundary not documented.

### Agent E — iOS UI + API client
- [MED] Weak `HKQuantityTypeIdentifier` prefix check.
- [MED] Unicode micro-glyph accidental handling.
- [MED] Relative URL path replacement.
- [MED] No HK write authorization pre-check.
- [MED] NSAllowsArbitraryLoads for production.
- [LOW] `compactMap` misuse.
- [LOW] Race on rapid Record toggles.
- [LOW] No parse/save loading spinners.
- [LOW] Case-sensitive meal_type strings.
- [LOW] No URLSession timeout / cancellation.
