import SwiftUI

/// Physical Galaxy firmware, kept separate from bootable emulator versions.
struct SamsungView: View {
    @State private var models: [SamsungChoice] = []
    @State private var countries: [SamsungChoice] = []
    @State private var firmware: [SamsungFirmware] = []
    @State private var query = ""
    @State private var modelID: String?
    @State private var countryID = ""
    @State private var page: UInt32 = 1
    @State private var hasMore = false
    @State private var loading = false
    @State private var status = ""
    @State private var generation = 0

    private var filtered: [SamsungChoice] {
        models.filter { query.isEmpty || $0.label.localizedCaseInsensitiveContains(query) || $0.id.localizedCaseInsensitiveContains(query) }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Samsung Galaxy Firmware — UK").font(.headline).accessibilityAddTraits(.isHeader)
            Text("Firmware for physical Samsung devices. These files cannot run in AAE's emulator.")
            TextField("Search models", text: $query)
                .onSubmit { if !loading && query.trimmingCharacters(in: .whitespaces).count >= 3 { loadModels() } }
            List(filtered, id: \.id, selection: $modelID) { item in
                Text(item.label).tag(item.id)
            }
            .accessibilityLabel("Samsung models")
            .frame(minHeight: 150)
            Picker("UK carrier or product code", selection: $countryID) {
                Text("Choose a UK carrier or product code").tag("")
                ForEach(countries, id: \.id) { Text($0.label).tag($0.id) }
            }
            .disabled(countries.isEmpty)
            List(firmware, id: \.url) { item in
                VStack(alignment: .leading) {
                    Text("\(item.version), \(item.build)")
                    Text("\(item.model), \(item.country), \(item.productCode)")
                    if let url = URL(string: item.url) {
                        Link("View firmware download on SamMobile", destination: url)
                    }
                }
                .accessibilityElement(children: .contain)
            }
            .accessibilityLabel("Samsung firmware versions")
            .frame(minHeight: 180)
            Text(status).accessibilityLabel(status)
            HStack {
                Button("Search UK models") { loadModels() }.disabled(loading || query.trimmingCharacters(in: .whitespaces).count < 3)
                Button("Load more firmware") { loadFirmware(more: true) }.disabled(loading || !hasMore)
                Link("Open SamMobile", destination: URL(string: "https://www.sammobile.com/firmwares/archive/")!)
            }
        }
        .padding().frame(minWidth: 620, minHeight: 600)
        .onAppear { if models.isEmpty { status = "Enter a model number or phone name, then choose Search UK models. Only verified UK variants are shown." } }
        .onChange(of: modelID) { _ in loadCountries() }
        .onChange(of: countryID) { _ in loadFirmware(more: false) }
    }

    private func loadModels() {
        generation += 1
        let request = generation
        let search = query
        models = []; countries = []; firmware = []; modelID = nil; countryID = ""; hasMore = false
        loading = true
        status = "Checking UK Samsung models…"
        Task { @MainActor in
            defer { if request == generation { loading = false } }
            do {
                let result = try await samsungModels(query: search)
                guard request == generation else { return }
                models = result
                status = result.isEmpty ? "No matching UK Samsung models were found." : "\(result.count) verified UK models. Select a model."
            } catch { if request == generation { status = error.localizedDescription } }
        }
    }

    private func loadCountries() {
        guard let id = modelID else { return }
        generation += 1
        let request = generation
        countries = []; countryID = ""; firmware = []; hasMore = false
        guard !id.isEmpty else { loading = false; return }
        loading = true; status = "Reading UK carriers and product codes…"
        Task { @MainActor in
            defer { if request == generation { loading = false } }
            do {
                let result = try await samsungCountries(model: id)
                guard request == generation else { return }
                countries = result
                status = result.isEmpty ? "No UK firmware regions are listed for this model." : "Choose a UK carrier or product code."
            } catch { if request == generation { status = error.localizedDescription } }
        }
    }

    private func loadFirmware(more: Bool) {
        // Clearing the country while a model request runs must not invalidate it.
        guard let modelID, !countryID.isEmpty else { return }
        generation += 1
        let request = generation, id = modelID, country = countryID
        let nextPage: UInt32 = more ? page + 1 : 1
        if !more { firmware = []; hasMore = false }
        loading = true; status = "Reading firmware versions…"
        Task { @MainActor in
            defer { if request == generation { loading = false } }
            do {
                let result = try await samsungFirmwares(model: id, country: country, page: nextPage)
                guard request == generation else { return }
                let existing = Set(firmware.map(\.url))
                let fresh = result.filter { !existing.contains($0.url) }
                firmware += fresh
                page = nextPage; hasMore = !fresh.isEmpty
                status = "\(firmware.count) firmware builds. Versions are shown as SamMobile supplies them."
            } catch { if request == generation { status = error.localizedDescription } }
        }
    }
}

struct SamsungMenuItem: View {
    @Environment(\.openWindow) private var openWindow
    var body: some View {
        Button("Samsung Galaxy Firmware — UK…") { openWindow(id: "samsung") }
    }
}
