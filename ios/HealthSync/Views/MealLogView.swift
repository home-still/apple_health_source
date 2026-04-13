import HealthKit
import SwiftUI

@MainActor
struct MealLogView: View {
    @State private var mealType: String = "Lunch"
    @State private var transcript: String = ""
    @State private var isRecording: Bool = false
    @State private var isParsing: Bool = false
    @State private var isSaving: Bool = false
    @State private var response: MealNutritionResponse?
    @State private var statusMessage: String?
    @State private var errorMessage: String?

    private let speech = SpeechService()
    private let mealTypes = ["Breakfast", "Lunch", "Dinner", "Snack"]

    var body: some View {
        Form {
            Section("Meal") {
                Picker("Type", selection: $mealType) {
                    ForEach(mealTypes, id: \.self) { Text($0) }
                }
                .pickerStyle(.segmented)
            }

            Section("What did you eat?") {
                TextEditor(text: $transcript)
                    .frame(minHeight: 80)
                    .overlay(alignment: .topLeading) {
                        if transcript.isEmpty {
                            Text("Tap the mic and describe your meal…")
                                .foregroundStyle(.secondary)
                                .padding(.top, 8)
                                .padding(.leading, 4)
                                .allowsHitTesting(false)
                        }
                    }

                HStack {
                    Button {
                        Task { await toggleRecording() }
                    } label: {
                        Label(
                            isRecording ? "Stop" : "Record",
                            systemImage: isRecording ? "stop.circle.fill" : "mic.circle.fill"
                        )
                    }
                    .tint(isRecording ? .red : .blue)

                    Spacer()

                    Button("Parse") {
                        Task { await parse() }
                    }
                    .disabled(transcript.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || isParsing)
                }
            }

            if let response {
                Section("Matched foods") {
                    ForEach(Array(response.items.enumerated()), id: \.offset) { _, item in
                        VStack(alignment: .leading, spacing: 4) {
                            Text(item.matchedFood?.name ?? item.parsed.foodName)
                                .font(.subheadline.bold())
                            Text(itemDescription(item))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }

                Section("Totals") {
                    ForEach(response.totals, id: \.hkIdentifier) { n in
                        HStack {
                            Text(displayName(for: n.hkIdentifier))
                            Spacer()
                            Text("\(n.amount, specifier: "%.1f") \(n.unit.lowercased())")
                                .foregroundStyle(.secondary)
                                .monospacedDigit()
                        }
                        .font(.caption)
                    }
                }

                Section {
                    Button {
                        Task { await save(response) }
                    } label: {
                        Label("Save to Health", systemImage: "heart.text.square")
                    }
                    .disabled(isSaving)
                }
            }

            if let statusMessage {
                Section { Text(statusMessage).foregroundStyle(.green).font(.caption) }
            }
            if let errorMessage {
                Section { Text(errorMessage).foregroundStyle(.red).font(.caption) }
            }
        }
        .navigationTitle("Log Meal")
        .toolbar {
            ToolbarItem(placement: .navigationBarTrailing) {
                NavigationLink(destination: MealHistoryView()) {
                    Image(systemName: "clock.arrow.circlepath")
                }
                .accessibilityLabel("Meal history")
            }
        }
        .task {
            _ = await SpeechService.requestAuthorization()
        }
    }

    private func toggleRecording() async {
        if isRecording {
            await speech.stop()
            isRecording = false
            return
        }

        errorMessage = nil
        statusMessage = nil
        do {
            let stream = try await speech.transcribe()
            isRecording = true
            for try await text in stream {
                transcript = text
            }
            isRecording = false
        } catch {
            errorMessage = error.localizedDescription
            isRecording = false
        }
    }

    private func parse() async {
        errorMessage = nil
        statusMessage = nil
        isParsing = true
        defer { isParsing = false }
        do {
            let result = try await APIClient.shared.parseMeal(text: transcript, mealType: mealType)
            response = result
        } catch {
            errorMessage = error.localizedDescription
        }
    }

    private func save(_ payload: MealNutritionResponse) async {
        isSaving = true
        defer { isSaving = false }

        let foodName = payload.items
            .map { $0.matchedFood?.name ?? $0.parsed.foodName }
            .joined(separator: ", ")

        do {
            let result = try await HKManager.shared.writeMealCorrelation(
                foodName: foodName,
                mealType: payload.mealType,
                nutrients: payload.asHealthKitSamples(),
                syncIdentifier: payload.syncIdentifier
            )
            if result.skipped.isEmpty {
                statusMessage = "Saved \(result.written) nutrients to Apple Health."
            } else {
                statusMessage = """
                    Saved \(result.written); \(result.skipped.count) \
                    nutrient\(result.skipped.count == 1 ? "" : "s") skipped \
                    — write access not granted.
                    """
            }
            response = nil
            transcript = ""
        } catch {
            errorMessage = "Save failed: \(error.localizedDescription)"
        }
    }

    private func displayName(for hkIdentifier: String) -> String {
        hkIdentifier
            .replacingOccurrences(of: "HKQuantityTypeIdentifierDietary", with: "")
    }

    private func itemDescription(_ item: MatchedItem) -> String {
        let q = String(format: "%.1f", item.parsed.quantity)
        var base = "\(q) \(item.parsed.unit)"
        if let grams = item.grams {
            base += String(format: " · %.0f g", grams)
        }
        return base
    }
}

#Preview {
    NavigationStack { MealLogView() }
}
