# HealthKit → Rust Axum → Postgres 17: Agent Development Guide

> Target: macOS (MacBook Air M5), VSCode + SweetPad, Swift/SwiftUI iOS app, Rust Axum API, Postgres 17

---

## 1. Architecture Overview

```
┌──────────────────┐       HTTPS/JSON        ┌──────────────────┐       SQLx        ┌──────────────┐
│  iOS App (Swift)  │ ──────────────────────► │  Rust Axum API   │ ───────────────► │  Postgres 17  │
│  HealthKit reads  │  JWT auth + batch sync  │  (axum 0.8+)     │                  │  TimescaleDB? │
│  SwiftUI frontend │ ◄────────────────────── │  tokio runtime    │ ◄─────────────── │               │
└──────────────────┘                          └──────────────────┘                  └──────────────┘
        │
        │ HKAnchoredObjectQuery
        │ Background Delivery
        ▼
   Apple HealthKit
   (on-device only)
```

**Key constraint:** HealthKit has NO server-side API. All data lives on-device. Your iOS app must read it locally, then POST it to your Axum API.

---

## 2. Dev Environment Setup

### 2.1 Prerequisites

```bash
# Xcode (REQUIRED — SweetPad wraps xcodebuild under the hood)
xcode-select --install
# Then install full Xcode from App Store (needed for simulator + signing)

# Homebrew
/bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"

# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable

# Postgres 17
brew install postgresql@17
brew services start postgresql@17

# SQLx CLI (for migrations)
cargo install sqlx-cli --features postgres

# Swift tooling for VSCode
brew install swift-format swiftlint xcode-build-server
```

### 2.2 VSCode Extensions

| Extension | Purpose |
|---|---|
| **SweetPad** (`sweetpad.sweetpad`) | Build, run, debug iOS apps from VSCode via xcodebuild |
| **Swift** (Swift Server Work Group) | LSP, syntax, diagnostics |
| **CodeLLDB** | Swift/Rust debugging via LLDB |
| **rust-analyzer** | Rust LSP |
| **Even Better TOML** | Cargo.toml editing |
| **Swift Test Explorer** | Run XCTests from VSCode |

### 2.3 SweetPad Workflow

1. Create project in Xcode first (File → New → Project → iOS App, SwiftUI, Swift).
2. Enable HealthKit capability in Xcode (Signing & Capabilities tab).
3. Open the project folder in VSCode.
4. Run command palette: `SweetPad: Generate Build Server Config` — creates `buildServer.json`.
5. Select scheme + device in SweetPad panel.
6. `SweetPad: Build & Run (launch)` or press F5 with the LLDB launch config.

**Critical:** You still need Xcode installed. SweetPad delegates to `xcodebuild`. VSCode is your editor; Xcode is your toolchain. You'll return to Xcode only for:
- Adding entitlements/capabilities (HealthKit, Background Modes)
- Managing signing profiles
- Interface Builder (if used — not needed with pure SwiftUI)

---

## 3. iOS App: HealthKit Integration

### 3.1 Required Entitlements & Info.plist

In Xcode → Signing & Capabilities:
- Add **HealthKit** capability
- Check **Background Delivery** under HealthKit
- Add **Background Modes** → check "Background fetch"

In `Info.plist`:
```xml
<key>NSHealthShareUsageDescription</key>
<string>We read your health data to sync it to your personal dashboard.</string>
<key>NSHealthUpdateUsageDescription</key>
<string>We write workout summaries back to Apple Health.</string>
```

### 3.2 Extractable HealthKit Data Types (Comprehensive)

**Lesson:** HealthKit types fall into 4 families: Quantity, Category, Characteristic, and Composite (Correlation/Workout). You request read/write access per-type. Apple will reject your app if you request types you don't use — be greedy but justify each one.

#### Quantity Types (time-series numeric samples)

| Category | Identifiers |
|---|---|
| **Body Measurements** | `bodyMass`, `bodyMassIndex`, `bodyFatPercentage`, `height`, `leanBodyMass`, `waistCircumference` |
| **Fitness** | `stepCount`, `distanceWalkingRunning`, `distanceCycling`, `distanceSwimming`, `distanceWheelchair`, `distanceDownhillSnowSports`, `pushCount`, `cyclingSpeed`, `cyclingPower`, `cyclingCadence`, `cyclingFunctionalThresholdPower`, `runningSpeed`, `runningPower`, `runningStrideLength`, `runningVerticalOscillation`, `runningGroundContactTime`, `physicalEffort`, `estimatedWorkoutEffortScore` |
| **Activity** | `basalEnergyBurned`, `activeEnergyBurned`, `flightsClimbed`, `nikeFuel`, `appleExerciseTime`, `appleMoveTime`, `appleStandTime`, `swimmingStrokeCount`, `underwaterDepth` |
| **Vitals** | `heartRate`, `restingHeartRate`, `walkingHeartRateAverage`, `heartRateVariabilitySDNN`, `heartRateRecoveryOneMinute`, `oxygenSaturation`, `bodyTemperature`, `basalBodyTemperature`, `bloodPressureSystolic`, `bloodPressureDiastolic`, `respiratoryRate` |
| **Lab Results** | `bloodGlucose`, `electrodermalActivity`, `forcedExpiratoryVolume1`, `forcedVitalCapacity`, `inhalerUsage`, `insulinDelivery`, `peakExpiratoryFlowRate`, `numberOfTimesFallen` |
| **Nutrition** | `dietaryBiotin`, `dietaryCaffeine`, `dietaryCalcium`, `dietaryCarbohydrates`, `dietaryChloride`, `dietaryCholesterol`, `dietaryChromium`, `dietaryCopper`, `dietaryEnergyConsumed`, `dietaryFatMonounsaturated`, `dietaryFatPolyunsaturated`, `dietaryFatSaturated`, `dietaryFatTotal`, `dietaryFiber`, `dietaryFolate`, `dietaryIodine`, `dietaryIron`, `dietaryMagnesium`, `dietaryManganese`, `dietaryMolybdenum`, `dietaryNiacin`, `dietaryPantothenicAcid`, `dietaryPhosphorus`, `dietaryPotassium`, `dietaryProtein`, `dietaryRiboflavin`, `dietarySodium`, `dietarySugar`, `dietaryThiamin`, `dietaryVitaminA`, `dietaryVitaminB6`, `dietaryVitaminB12`, `dietaryVitaminC`, `dietaryVitaminD`, `dietaryVitaminE`, `dietaryVitaminK`, `dietaryWater`, `dietaryZinc` |
| **Audio/Environment** | `environmentalAudioExposure`, `environmentalSoundReduction`, `headphoneAudioExposure` |
| **UV** | `uvExposure` |

#### Category Types (enum-valued samples)

| Identifier | Values |
|---|---|
| `sleepAnalysis` | inBed, asleepUnspecified, asleepCore, asleepDeep, asleepREM, awake |
| `appleStandHour` | idle, stood |
| `menstrualFlow` | unspecified, light, medium, heavy, none |
| `ovulationTestResult` | negative, luteinizingHormoneSurge, estrogenSurge, indeterminate |
| `cervicalMucusQuality` | dry, sticky, creamy, watery, eggWhite |
| `intermenstrualBleeding` | (presence-only) |
| `sexualActivity` | (presence-only, optional metadata) |
| `mindfulSession` | (presence-only, start/end time matters) |
| `highHeartRateEvent` | (presence-only) |
| `lowHeartRateEvent` | (presence-only) |
| `irregularHeartRhythmEvent` | (presence-only) |
| `pregnancyTestResult` | negative, positive, indeterminate |
| `progesteroneTestResult` | negative, positive, indeterminate |
| `lowCardioFitnessEvent` | (presence-only) |
| `appleWalkingSteadinessEvent` | (presence-only) |

#### Characteristic Types (static, read-only)

- `biologicalSex`, `dateOfBirth`, `bloodType`, `fitzpatrickSkinType`, `wheelchairUse`, `activityMoveMode`

#### Composite Types

- **Workouts** (`HKWorkout`): activityType, duration, totalEnergyBurned, totalDistance, metadata, workout events, workout route (GPS via `HKWorkoutRoute`)
- **Correlations**: blood pressure (systolic + diastolic paired), food (nutrients grouped)
- **ActivitySummary** (`HKActivitySummary`): daily move/exercise/stand ring data
- **ECG** (`HKElectrocardiogram`): voltage measurements (read-only, cannot access raw waveform easily — only classification + symptoms)
- **Workout Routes** (`HKWorkoutRoute`): GPS `CLLocation` series
- **Vision Prescriptions** (`HKVisionPrescription`)

#### New in iOS 18/19 (2025+)

- **Medications API** (`HKUserAnnotatedMedication`): medication name, dosage, schedule, archived status. Uses per-object read authorization.
- **State of Mind** samples
- **GRF (Ground Reaction Force)** running metrics

### 3.3 App Architecture (Swift)

```
Sources/
├── App.swift                    # @main, AppDelegate adapter for background delivery
├── HealthKit/
│   ├── HKManager.swift          # Singleton: authorization, queries
│   ├── HKTypes.swift            # Registry of all types we request
│   ├── HKBackgroundDelivery.swift # Observer queries + enableBackgroundDelivery
│   └── HKAnchorStore.swift      # Persist HKQueryAnchor per type (UserDefaults or file)
├── Sync/
│   ├── SyncEngine.swift         # Batches samples → JSON → POST to API
│   ├── SyncQueue.swift          # Offline queue with retry (Core Data or file-backed)
│   └── APIClient.swift          # URLSession wrapper, JWT auth, endpoint config
├── Models/
│   ├── HealthSample.swift       # Codable struct matching API schema
│   └── SyncState.swift          # Last sync timestamp per data type
├── Views/
│   ├── DashboardView.swift
│   ├── SettingsView.swift
│   └── AuthView.swift
└── Utilities/
    └── DateFormatting.swift
```

### 3.4 Sync Strategy: Anchored Object Queries

**Lesson:** Don't poll with `HKSampleQuery`. Use `HKAnchoredObjectQuery` which gives you a delta (new samples + deleted samples) since a persisted anchor. This is efficient and idempotent.

```swift
// Pseudocode — the pattern every agent must follow
func syncType(_ sampleType: HKSampleType) {
    let anchor = anchorStore.load(for: sampleType) // nil on first run = full sync
    
    let query = HKAnchoredObjectQuery(
        type: sampleType,
        predicate: nil,
        anchor: anchor,
        limit: HKObjectQueryNoLimit
    ) { query, newSamples, deletedObjects, newAnchor, error in
        guard let samples = newSamples, let anchor = newAnchor else { return }
        
        // 1. Convert HKSample → [HealthSample] (your Codable model)
        // 2. POST batch to /api/v1/health/sync
        // 3. On success: anchorStore.save(anchor, for: sampleType)
        // 4. On failure: enqueue in SyncQueue for retry
    }
    healthStore.execute(query)
}
```

### 3.5 Background Delivery

```swift
// In AppDelegate.didFinishLaunchingWithOptions or App.init:
for type in allReadTypes {
    guard let sampleType = type as? HKSampleType else { continue }
    healthStore.enableBackgroundDelivery(
        for: sampleType,
        frequency: .immediate  // .hourly, .daily, .weekly also available
    ) { success, error in
        // .immediate: HealthKit wakes your app ASAP when new data arrives
        // Some types (like steps) may still batch to ~hourly minimum
    }
}
```

**Important caveats:**
- Background delivery wakes your app briefly — you get ~30s of execution time.
- Use `HKObserverQuery` as the trigger, then run an `HKAnchoredObjectQuery` to fetch the delta.
- Must call `completionHandler()` in the observer callback or iOS kills you.
- Must re-register observers on every app launch.
- Test on a REAL DEVICE. Simulator has no Health data and background delivery won't fire.

---

## 4. Rust Axum API

### 4.1 Project Structure

```
api/
├── Cargo.toml
├── .env                        # DATABASE_URL, JWT_SECRET, etc.
├── migrations/
│   ├── 001_create_users.sql
│   ├── 002_create_health_samples.sql
│   └── 003_create_sync_state.sql
├── src/
│   ├── main.rs
│   ├── config.rs               # Env parsing
│   ├── db.rs                   # PgPool setup
│   ├── error.rs                # AppError → axum::IntoResponse
│   ├── auth/
│   │   ├── mod.rs
│   │   ├── jwt.rs              # jsonwebtoken encode/decode
│   │   └── middleware.rs       # axum middleware layer
│   ├── routes/
│   │   ├── mod.rs
│   │   ├── health_check.rs
│   │   ├── auth_routes.rs
│   │   └── sync_routes.rs      # POST /api/v1/health/sync
│   ├── models/
│   │   ├── mod.rs
│   │   ├── user.rs
│   │   └── health_sample.rs    # Maps 1:1 to DB table
│   └── handlers/
│       ├── mod.rs
│       └── sync_handler.rs     # Upsert batch logic
```

### 4.2 Cargo.toml (Key Dependencies)

```toml
[package]
name = "healthsync-api"
version = "0.1.0"
edition = "2024"

[dependencies]
axum = { version = "0.8", features = ["macros"] }
tokio = { version = "1", features = ["full"] }
sqlx = { version = "0.8", features = ["runtime-tokio", "tls-rustls", "postgres", "chrono", "uuid", "json"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["v4", "serde"] }
tower-http = { version = "0.6", features = ["cors", "trace"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
jsonwebtoken = "9"
dotenvy = "0.15"
```

### 4.3 Database Schema (Postgres 17)

```sql
-- migrations/001_create_users.sql
CREATE EXTENSION IF NOT EXISTS "uuid-ossp";

CREATE TABLE users (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    email TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- migrations/002_create_health_samples.sql
-- One wide table, partitioned by sample_type for performance.
-- Each row is one HKSample.

CREATE TABLE health_samples (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id),
    
    -- HealthKit identity
    hk_uuid TEXT NOT NULL,               -- HKSample.uuid from device
    sample_type TEXT NOT NULL,            -- e.g. "HKQuantityTypeIdentifierStepCount"
    source_name TEXT,                     -- e.g. "Apple Watch", "MyFitnessPal"
    source_bundle_id TEXT,               -- e.g. "com.apple.health"
    device_name TEXT,
    device_model TEXT,
    
    -- Temporal
    start_date TIMESTAMPTZ NOT NULL,
    end_date TIMESTAMPTZ NOT NULL,
    
    -- Value (polymorphic)
    quantity_value DOUBLE PRECISION,      -- for quantity types
    quantity_unit TEXT,                    -- e.g. "count", "count/min", "kcal"
    category_value INT,                   -- for category types (enum int)
    
    -- Metadata (HealthKit metadata dict)
    metadata JSONB,
    
    -- Sync bookkeeping
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    
    UNIQUE(user_id, hk_uuid)             -- Idempotent upserts
);

CREATE INDEX idx_samples_user_type_date ON health_samples(user_id, sample_type, start_date DESC);
CREATE INDEX idx_samples_hk_uuid ON health_samples(hk_uuid);

-- migrations/003_create_workouts.sql
CREATE TABLE workouts (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id),
    hk_uuid TEXT NOT NULL,
    activity_type INT NOT NULL,           -- HKWorkoutActivityType raw value
    activity_name TEXT,
    duration_seconds DOUBLE PRECISION,
    total_energy_burned DOUBLE PRECISION,
    total_distance DOUBLE PRECISION,
    start_date TIMESTAMPTZ NOT NULL,
    end_date TIMESTAMPTZ NOT NULL,
    metadata JSONB,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, hk_uuid)
);

-- migrations/004_create_workout_routes.sql
CREATE TABLE workout_route_points (
    id BIGSERIAL PRIMARY KEY,
    workout_id UUID NOT NULL REFERENCES workouts(id) ON DELETE CASCADE,
    latitude DOUBLE PRECISION NOT NULL,
    longitude DOUBLE PRECISION NOT NULL,
    altitude DOUBLE PRECISION,
    horizontal_accuracy DOUBLE PRECISION,
    vertical_accuracy DOUBLE PRECISION,
    speed DOUBLE PRECISION,
    course DOUBLE PRECISION,
    timestamp TIMESTAMPTZ NOT NULL
);

CREATE INDEX idx_route_workout ON workout_route_points(workout_id, timestamp);

-- migrations/005_create_characteristics.sql
CREATE TABLE user_characteristics (
    user_id UUID PRIMARY KEY REFERENCES users(id),
    biological_sex TEXT,
    date_of_birth DATE,
    blood_type TEXT,
    fitzpatrick_skin_type TEXT,
    wheelchair_use BOOLEAN,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- migrations/006_create_activity_summaries.sql
CREATE TABLE activity_summaries (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v4(),
    user_id UUID NOT NULL REFERENCES users(id),
    date DATE NOT NULL,
    active_energy_burned DOUBLE PRECISION,
    active_energy_burned_goal DOUBLE PRECISION,
    exercise_time_minutes DOUBLE PRECISION,
    exercise_time_goal DOUBLE PRECISION,
    stand_hours INT,
    stand_hours_goal INT,
    synced_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE(user_id, date)
);
```

### 4.4 Sync Endpoint Pattern

```rust
// POST /api/v1/health/sync
// Body: { "samples": [...], "workouts": [...], "deleted_uuids": [...] }

async fn sync_handler(
    State(pool): State<PgPool>,
    claims: Claims,  // extracted from JWT middleware
    Json(payload): Json<SyncPayload>,
) -> Result<Json<SyncResponse>, AppError> {
    let mut tx = pool.begin().await?;
    
    // Upsert samples (ON CONFLICT DO UPDATE)
    for sample in &payload.samples {
        sqlx::query!(
            r#"INSERT INTO health_samples 
               (user_id, hk_uuid, sample_type, source_name, source_bundle_id,
                device_name, device_model, start_date, end_date,
                quantity_value, quantity_unit, category_value, metadata)
               VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
               ON CONFLICT (user_id, hk_uuid) DO UPDATE SET
                 quantity_value = EXCLUDED.quantity_value,
                 category_value = EXCLUDED.category_value,
                 metadata = EXCLUDED.metadata,
                 synced_at = NOW()"#,
            claims.user_id, sample.hk_uuid, sample.sample_type,
            /* ...remaining fields... */
        ).execute(&mut *tx).await?;
    }
    
    // Handle deletions
    for uuid in &payload.deleted_uuids {
        sqlx::query!(
            "DELETE FROM health_samples WHERE user_id = $1 AND hk_uuid = $2",
            claims.user_id, uuid
        ).execute(&mut *tx).await?;
    }
    
    tx.commit().await?;
    Ok(Json(SyncResponse { synced: payload.samples.len() }))
}
```

**Performance tip:** For large batches (1000+ samples), use `COPY` via `sqlx` raw or build a single multi-row INSERT instead of looping.

### 4.5 Running the API

```bash
# Terminal 1: Postgres
brew services start postgresql@17
createdb healthsync

# Terminal 2: Migrations
cd api
echo "DATABASE_URL=postgres://$(whoami)@localhost/healthsync" > .env
sqlx migrate run

# Terminal 3: Dev server
cargo watch -q -c -w src/ -x run
```

---

## 5. Agent Task Breakdown

Break development into these discrete, independently-testable tasks:

### Phase 1: Scaffolding
1. **Create Xcode project** with SwiftUI, HealthKit capability, background delivery entitlement
2. **Create Rust project** with Axum skeleton, health-check endpoint, PgPool, CORS
3. **Write and run all migrations** via sqlx-cli
4. **Configure VSCode** with SweetPad, verify build & run works on simulator

### Phase 2: API Core
5. **Auth routes**: register, login → JWT
6. **JWT middleware**: extract + validate claims
7. **Sync endpoint**: POST with upsert logic (start with just `stepCount`)
8. **Integration test**: curl/httpie against running API

### Phase 3: iOS HealthKit
9. **HKManager**: request authorization for all desired types
10. **Read samples**: `HKAnchoredObjectQuery` for one type, verify data
11. **Codable models**: `HealthSample` struct matching API contract
12. **APIClient**: URLSession POST to sync endpoint
13. **End-to-end**: read steps → sync to API → verify in Postgres

### Phase 4: Full Extraction
14. **Expand to all quantity types** (loop over registry)
15. **Category types** (sleep, mindful sessions, menstrual, etc.)
16. **Workouts + routes** (separate table, GPS extraction from `HKWorkoutRoute`)
17. **Characteristics** (one-time read of static data)
18. **Activity summaries** (ring data)

### Phase 5: Background Sync
19. **Background delivery**: observer queries + `enableBackgroundDelivery`
20. **Offline queue**: retry failed syncs on next wake
21. **Anchor persistence**: save per-type anchors to UserDefaults or Keychain

### Phase 6: Production Hardening
22. **Rate limiting** on API (tower middleware)
23. **Compression** (gzip request bodies for large batches)
24. **Pagination** for API reads (if building a dashboard)
25. **Docker compose** for API + Postgres
26. **CI**: GitHub Actions for Rust tests + Swift tests

---

## 6. Key Gotchas & Lessons

1. **No iPad support.** HealthKit is iPhone (and Apple Watch) only. `HKHealthStore.isHealthDataAvailable()` returns false on iPad.

2. **Simulator has no health data.** You must test on a real iPhone. You can add synthetic data in the Health app on the simulator manually, but background delivery won't fire.

3. **Permission denials are silent.** If a user denies read access for a type, queries return empty — not errors. Your app cannot distinguish "no data" from "denied." This is by design for privacy.

4. **Request only what you use.** Apple App Review will reject you if you request 50 types and only use 3. Justify each type in your review notes.

5. **Anchors must persist across launches.** If you lose your anchor, you'll re-sync everything. Store them in UserDefaults keyed by type identifier.

6. **HKSample.uuid is your idempotency key.** Every HealthKit sample has a stable UUID. Use it as your `UNIQUE(user_id, hk_uuid)` constraint to make syncs idempotent.

7. **Background delivery gives you ~30 seconds.** Do a quick delta query + HTTP POST. If the network is down, queue locally and retry later.

8. **Xcode is still required.** SweetPad/VSCode is your editor, but `xcodebuild` (bundled with Xcode) does the compiling. You need Xcode installed.

9. **ECG raw waveform data is restricted.** You can read the classification (sinus rhythm, AFib, etc.) but not the raw voltage array without special Apple approval.

10. **Medications API (iOS 18+) uses per-object auth.** It's a different authorization flow than standard HealthKit — `requestPerObjectReadAuthorization`.

---

## 7. Reference Links

- [Apple HealthKit Documentation](https://developer.apple.com/documentation/healthkit)
- [SweetPad GitHub](https://github.com/sweetpad-dev/sweetpad)
- [SweetPad Docs](https://sweetpad.hyzyla.dev)
- [Axum GitHub](https://github.com/tokio-rs/axum)
- [SQLx GitHub](https://github.com/launchbadge/sqlx)
- [HKAnchoredObjectQuery](https://developer.apple.com/documentation/healthkit/hkanchoredobjectquery)
- [Background Delivery Entitlement](https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.developer.healthkit.background-delivery)
- [VSCode iOS Setup (Kulman)](https://blog.kulman.sk/vscode-ios-setup/)
