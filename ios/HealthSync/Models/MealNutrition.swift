import Foundation
import HealthKit

struct MealParseRequest: Codable {
    let text: String
    let mealType: String

    enum CodingKeys: String, CodingKey {
        case text
        case mealType = "meal_type"
    }
}

struct MealNutritionResponse: Codable {
    let syncIdentifier: UUID
    let mealType: String
    let items: [MatchedItem]
    let totals: [NutrientValue]

    enum CodingKeys: String, CodingKey {
        case syncIdentifier = "sync_identifier"
        case mealType = "meal_type"
        case items
        case totals
    }
}

struct MatchedItem: Codable {
    let parsed: ParsedItem
    let matchedFood: MatchedFood?
    let grams: Double?
    let nutrients: [NutrientValue]

    enum CodingKeys: String, CodingKey {
        case parsed
        case matchedFood = "matched_food"
        case grams
        case nutrients
    }
}

struct ParsedItem: Codable {
    let foodName: String
    let quantity: Double
    let unit: String
    let preparationMethod: String?
    let confidence: String
    let databaseSearchTerms: [String]

    enum CodingKeys: String, CodingKey {
        case foodName = "food_name"
        case quantity
        case unit
        case preparationMethod = "preparation_method"
        case confidence
        case databaseSearchTerms = "database_search_terms"
    }
}

struct MatchedFood: Codable {
    let fdcId: Int
    let name: String
    let dataType: String

    enum CodingKeys: String, CodingKey {
        case fdcId = "fdc_id"
        case name
        case dataType = "data_type"
    }
}

struct NutrientValue: Codable {
    let hkIdentifier: String
    let unit: String
    let amount: Double
    let sparse: Bool

    enum CodingKeys: String, CodingKey {
        case hkIdentifier = "hk_identifier"
        case unit
        case amount
        case sparse
    }
}

struct MealHistoryEntry: Codable, Identifiable {
    let id: UUID
    let syncIdentifier: UUID
    let rawText: String
    let mealType: String
    let finalNutrients: [NutrientValue]
    let createdAt: Date

    enum CodingKeys: String, CodingKey {
        case id
        case syncIdentifier = "sync_identifier"
        case rawText = "raw_text"
        case mealType = "meal_type"
        case finalNutrients = "final_nutrients"
        case createdAt = "created_at"
    }
}

struct MealHistoryResponse: Codable {
    let items: [MealHistoryEntry]
}

extension MealNutritionResponse {
    /// Flatten per-item nutrients into a single map keyed by HealthKit identifier,
    /// ready to pass to `HKManager.writeMealCorrelation`. USDA unit strings are
    /// translated to `HKUnit` values.
    func asHealthKitSamples() -> [HKQuantityTypeIdentifier: (amount: Double, unit: HKUnit)] {
        var out: [HKQuantityTypeIdentifier: (Double, HKUnit)] = [:]
        for n in totals {
            guard let id = hkIdentifier(from: n.hkIdentifier),
                  let unit = hkUnit(from: n.unit) else {
                continue
            }
            out[id] = (n.amount, unit)
        }
        return out
    }
}

private func hkIdentifier(from raw: String) -> HKQuantityTypeIdentifier? {
    guard raw.hasPrefix("HKQuantityTypeIdentifier") else { return nil }
    return HKQuantityTypeIdentifier(rawValue: raw)
}

private func hkUnit(from raw: String) -> HKUnit? {
    switch raw.uppercased() {
    case "KCAL": return .kilocalorie()
    case "G", "GRAM", "GRAMS": return .gram()
    case "MG": return .gramUnit(with: .milli)
    case "UG", "ΜG", "MCG": return .gramUnit(with: .micro)
    case "IU": return .internationalUnit()
    case "ML": return .literUnit(with: .milli)
    case "L": return .liter()
    default: return nil
    }
}
