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
            Text("Samsung Galaxy Firmware").font(.headline).accessibilityAddTraits(.isHeader)
            Text("Firmware for physical Samsung devices. These files cannot run in AAE's emulator.")
            TextField("Search models", text: $query)
            List(filtered, id: \.id, selection: $modelID) { item in
                Text(item.label).tag(item.id)
            }
            .accessibilityLabel("Samsung models")
            .frame(minHeight: 150)
            Picker("Country or carrier", selection: $countryID) {
                Text("Choose a country or carrier").tag("")
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
                Button("Refresh models") { loadModels() }.disabled(loading)
                Button("Load more firmware") { loadFirmware(more: true) }.disabled(loading || !hasMore)
                Link("Open SamMobile", destination: URL(string: "https://www.sammobile.com/firmwares/archive/")!)
            }
        }
        .padding().frame(minWidth: 620, minHeight: 600)
        .onAppear { if models.isEmpty { loadModels() } }
        .onChange(of: modelID) { _ in loadCountries() }
        .onChange(of: countryID) { _ in loadFirmware(more: false) }
    }

    private func loadModels() {
        generation += 1
        let request = generation
        loading = true
        status = "Reading Samsung models…"
        Task { @MainActor in
            defer { if request == generation { loading = false } }
            do {
                let result = try await samsungModels()
                guard request == generation else { return }
                models = result
                status = "\(result.count) models from SamMobile. Search or select a model."
            } catch { if request == generation { status = error.localizedDescription } }
        }
    }

    private func loadCountries() {
        generation += 1
        let request = generation
        let id = modelID
        countries = []; countryID = ""; firmware = []; hasMore = false
        guard let id, !id.isEmpty else { loading = false; return }
        loading = true; status = "Reading countries and carriers…"
        Task { @MainActor in
            defer { if request == generation { loading = false } }
            do {
                let result = try await samsungCountries(model: id)
                guard request == generation else { return }
                countries = result
                status = result.isEmpty ? "No countries are listed for this model." : "Choose a country or carrier."
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
        Button("Samsung Galaxy Firmware…") { openWindow(id: "samsung") }
    }
}
