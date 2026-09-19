# Oficjalny klient BetterDesk (desktop)

Ten fork ([BetterDesk-Client](../)) jest **oficjalnym klientem desktop BetterDesk**:

1. **Nie łączy się** z publiczną infrastrukturą rustdesk.com.
2. Użytkownik **wpisuje** ID / Relay / API / Key (Settings → Network) lub importuje deploy string z panelu.
3. Klient **identyfikuje się** wobec API jako `betterdesk-desktop` (`device_type` / `client_product`) oraz jawnym `product_sku` (`betterdesk-desktop` | `betterdesk-support`).
4. Mechanizmy bake-in (`custom.txt`, MSI template, deploy string) są zachowane pod **Client Generator** w panelu web (dwa SKU — dualizm poniżej).

## Kill public cloud

| Warstwa | Zachowanie |
|---------|------------|
| `RENDEZVOUS_SERVERS` | puste — brak `rs-ny.rustdesk.com` |
| `get_api_server_` | pusty string zamiast `admin.rustdesk.com` |
| `get_key` | brak fallbacku do publicznego `RS_PUB_KEY` |
| Update check | URL pusty — nie woła `api.rustdesk.com` |
| Connect `id@public` | zablokowane / ignorowane |

Bez skonfigurowanego serwera klient **nie rejestruje się** na publicznym cloudu. Status bar pokazuje tip → otwiera Settings → Network.

## Ręczna konfiguracja (produkt codzienny)

1. Panel BetterDesk → Dashboard → **RustDesk Client Configuration**.
2. W kliencie: Settings → Network → ID / Relay / API / Key (lub **Import** deploy string).
3. API: `http://<host>:21114` (native) lub `:21121` (Docker AIO); klucz = `id_ed25519.pub`.

Deploy string = `reverse(base64({host,relay,api,key}))` — ten sam format co stock RustDesk / Dashboard.

## Identyfikacja klienta

`get_sysinfo()`, heartbeat i enrollment wysyłają m.in.:

- `app_name` — `BetterDesk Client` / `BetterDesk Support Agent` (lub z `custom.txt`)
- exe / proces — `betterdesk` (`betterdesk.exe` na Windows)
- `client` / `client_product` / `device_type` — zawsze `betterdesk-desktop` (`BETTERDESK_CLIENT_PRODUCT`; legacy CDAP-safe)
- `product_sku` — `betterdesk-desktop` (pełny) albo `betterdesk-support` (bake-in incoming-only)
- `conn_mode` — `normal` albo `incoming-only` (wynik `HARD_SETTINGS`, nie życzenie serwera)
- opcjonalnie `enrollment_status`, `branding_revision`, `branding_source`
- `license` — `AGPL-3.0-only`
- `upstream_project` / `upstream_repo` — RustDesk + `https://github.com/rustdesk/rustdesk`
- `source_repo` — `https://github.com/UNITRONIX/BetterDesk-Client`

Helper HTTP: [`src/hbbs_http/betterdesk.rs`](../src/hbbs_http/betterdesk.rs) (`/api/health`, `/api/branding`, `/api/server-key`, identity helpers).

## Dualizm SKU (Full Client vs Support Agent)

Generator w panelu buduje **dwa** artefakty z tego samego binariów + różnego `custom.txt`. **Nie ma** zdalnego przełączania full ↔ support.

| SKU | Generator / bake-in | Tryb pulpitu | Serwery / klucz | Branding | Strategy / API config |
|-----|---------------------|--------------|-----------------|----------|------------------------|
| **BetterDesk Client** (pełny) | `examples/betterdesk-custom.example.json` lub profil full | Normalny (outbound + inbound) | `default-settings` lub fleet `override-settings` | `GET /api/branding` | Heartbeat `strategy` + enrollment + sysinfo |
| **BetterDesk Support Agent** | `product_type: betterdesk-support` / [`betterdesk-support-agent.example.json`](../examples/betterdesk-support-agent.example.json) | **Na stałe** `conn-type: incoming` | Sztywno w `override-settings` + `hide-server-settings` | Ten sam `/api/branding` | Branding + enrollment/status; **bez** wyjścia z incoming-only |

**Enforcement (panel / API — poza tym repo):**

- Przy register/heartbeat: jeśli `product_sku=betterdesk-support` (lub tagi `betterdesk-support,incoming-only`), oznacz urządzenie jako Support Agent.
- Nie wysyłaj strategii usuwającej incoming-only ani zmieniającej serwery floty Support w sposób conflicting.
- UI panelu nie oferuje „przełącz na full”; device details pokazują `product_sku`, `conn_mode`, enrollment, branding revision.

**Enforcement (klient):** `conn-type` tylko w `HARD_SETTINGS`; heartbeat `strategy` pomija opcje fixed (`override-settings`) oraz na Support Agent klucze sieciowe / lockdown.

## Client Branding (runtime, z API)

Po poprawnej konfiguracji Network (`api-server`) oficjalny klient **automatycznie** pobiera branding z publicznego `GET {api}/api/branding` (bez loginu konta, bez specjalnych uprawnień).

| Zachowanie | Szczegóły |
|------------|-----------|
| Trigger | pętla `hbbs_http::sync` (po heartbeat / gdy API skonfigurowane) |
| Persist | LocalConfig `branding-*` + plik logo; `branding-source=server` |
| Clear | gdy usunięto/zmieniono `api-server`, albo serwer ma `revision=0` (nigdy nie zapisano w panelu) |
| UI | Settings → Branding w trybie managed (read-only) gdy `branding-source=server` |
| Stock RustDesk | **nie** woła `/api/branding`; dostaje tylko bezpieczny podzbiór przez heartbeat `strategy.config_options` (np. `display-name`) |

Źródło prawdy edytuje panel: **Main → Client Branding** (nie Settings → Branding konsoli). Kontrakt: `schema_version`, `revision`, pola firmy/kontaktu/logo, `profiles.betterdesk` / `profiles.rustdesk`.

## Attribution / licencja (AGPL-3.0)

BetterDesk Client **jest forkiem klienta RustDesk**. W Settings → About oraz w sysinfo:

- licencja **AGPL-3.0** (plik `LICENSE` w repo)
- link do **kodu źródłowego forka** (`BetterDesk-Client`)
- link do **upstream** (`rustdesk/rustdesk`)
- copyright UNITRONIX + oryginalnych autorów RustDesk / Purslane

Stałe: `UPSTREAM_*` / `FORK_REPO_URL` / `LICENSE_*` w [`libs/hbb_common/src/config.rs`](../libs/hbb_common/src/config.rs) oraz [`flutter/lib/consts.dart`](../flutter/lib/consts.dart).

## Bake-in pod Generator (panel)

| Mechanizm | Rola |
|-----------|------|
| **Plain JSON `custom.txt`** | Phase A — plik zaczyna się od `{`; `default-settings` / `override-settings` |
| **Signed `custom.txt`** | Phase B — base64(NaCl-sign(JSON)); pubkey w `res/betterdesk/custom-client-signing.pub` |
| **MSI template** | [`res/msi/preprocess.py`](../res/msi/preprocess.py) — cab2 z `custom.txt` + branding |
| **Deploy string** | Dashboard / Import — bez przebudowy binarki |
| **Exe license** | `host=`,`key=`,`api=`,`relay=` w nazwie pliku (portable) |

### Generowanie kluczy podpisujących

```bash
python scripts/generate_custom_client_signing_key.py
python scripts/sign_custom_client_config.py examples/betterdesk-custom.example.json > custom.txt
```

Seed (`*.seed`) jest prywatny — trafia do panelu Generatora (jak Support Agent `bundleSigningKey`), **nie** do publicznych mirrorów.

### Przykład `default-settings` (user może zmienić Network)

```json
{
  "app-name": "BetterDesk Client",
  "default-settings": {
    "custom-rendezvous-server": "desk.example.com",
    "relay-server": "desk.example.com",
    "api-server": "http://desk.example.com:21114",
    "key": "<id_ed25519.pub contents>"
  }
}
```

Fleet lock: te same pola w `override-settings` (+ opcjonalnie `hide-server-settings`).

## Support Agent (incoming-only, jak QuickSupport)

Support Agent to **osobny SKU** Generatora (patrz dualizm powyżej): użytkownik widzi **ID + hasło**, **nie łączy się** do innych urządzeń, a technik łączy się z **pełnego** BetterDesk desktop **do** agenta. Zdalny pulpit (input) jest jednokierunkowy (controller → host); chat / schowek / pliki mogą działać w obie strony wg uprawnień sesji.

Nie wymaga nowego protokołu — hard setting `conn-type: incoming` (`is_incoming_only()` w [`libs/hbb_common/src/config.rs`](../libs/hbb_common/src/config.rs); blokada outbound w [`src/client.rs`](../src/client.rs); UI w [`flutter/lib/desktop/pages/desktop_home_page.dart`](../flutter/lib/desktop/pages/desktop_home_page.dart)).

### Bake-in SKU

1. Zbuduj desktop jak w [BUILD_DESKTOP.md](BUILD_DESKTOP.md) albo użyj release `desktop/*` z GitHub Actions.
2. Skopiuj / podpisz przykład: [`examples/betterdesk-support-agent.example.json`](../examples/betterdesk-support-agent.example.json) → `custom.txt` obok `betterdesk.exe`.
3. Albo **panel BetterDesk → Generator**: instalacja modułu szablonów z Releases Client, potem profil multi-platform (Windows / Linux / macOS) z `product_type: betterdesk-support`.

Top-level (→ `HARD_SETTINGS`):

| Klucz | Wartość | Efekt |
|-------|---------|--------|
| `conn-type` | `incoming` | Ukrywa Connect UI; blokuje outbound (wyjątek: verified switch-sides); `product_sku=betterdesk-support` |
| `disable-settings` | `Y` | Ukrywa Settings (opcjonalny lockdown floty) |

W `override-settings`: serwery + `key`, `hide-server-settings`, `hide-help-cards`.

### Runtime branding + enrollment

Z bake-in `api-server` (oba SKU):

- Branding: `GET /api/branding` (~60 s) — panel **Main → Client Branding**
- Enrollment: `POST /api/devices/register` z `device_type=betterdesk-desktop` (legacy) + `product_sku` / `conn_mode` / `tags` (managed → kolejka akceptacji; open → od razu)

### CI: czyste klienty + szablony Generatora

Jedyny workflow release: [`.github/workflows/betterdesk-desktop-release.yml`](../.github/workflows/betterdesk-desktop-release.yml) (wołany build: [`flutter-build.yml`](../.github/workflows/flutter-build.yml) — **tylko desktop**).

| Trigger | Zachowanie |
|---------|------------|
| Push na `master` (po aktualizacji) | Build + **development prerelease** `desktop/<ver>-dev.<run>+<sha>` z changelogiem |
| Tag `desktop/*` | Build + prerelease pod tym tagiem |
| `workflow_dispatch` | Ręczny build / tag |

**W CI budowany jest tylko pełny BetterDesk Client** (`product_sku=betterdesk-desktop`, bez `custom.txt`, **bez** `conn-type: incoming`). **Support Agent nie ma osobnego joba** — powstaje w panelu (Generator + bake-in), nie w GitHub Actions.

Zakres platform: **Windows / Linux / macOS** (x86_64 + aarch64). Brak jobów Android, iOS, AppImage, Flatpak, Sciter, DRM, web, F-Droid, nightly/tag upstream.

| Asset | Zawartość |
|-------|-----------|
| `betterdesk-*-windows-*.tar.gz` / `.exe` / `.msi` | Portable + instalatory Windows |
| `betterdesk-*-linux-*.deb` / `.rpm` / `*-portable.tar.gz` | Instalatory + portable Linux |
| `betterdesk-*-macos-*.dmg` / `.tar.gz` | DMG + archiwum macOS |
| `generator-templates-*.tar.gz` + manifest | Czyste binaria pod konsolowy Generator |
| `betterdesk-template-*.msi` | MSI template (cab2) |

Pack lokalnie: `python scripts/pack_generator_templates.py --dist-root ./dist --out ./generator-templates --version … --archive`.
Walidacja: `python scripts/validate_generator_templates.py --root ./generator-templates`.
Manifest generatora wymaga kompletu Windows/Linux/macOS x86_64+aarch64, binariów bez `custom.txt`, markerów injectowania i hashy archiwów. Paczka zawiera także plik `.sha256` dla archiwum generatora.

### Baseline bezpieczeństwa floty

- Produkcja: **signed** `custom.txt` (Phase B), nie plain JSON; seed tylko na konsoli (`BETTERDESK_CUSTOM_CLIENT_SIGNING_SEED`).
- Release klienta odrzuca unsigned plain JSON; plain JSON pozostaje dostępny wyłącznie dla buildów debug.
- Serwery w `override-settings` + `hide-server-settings` / `disable-settings`.
- Preferuj HTTPS dla `api-server` gdy panel to umożliwia; `/api/branding` jest publicznym GET (kosmetyka — zaufanie = Twój API).
- Technik używa pełnego klienta (bez `conn-type: incoming`).
- Support bundle może opcjonalnie dołączyć skrypty instalacji usługi/autostartu dla Windows, Linux i macOS. Opcje są wyłączone domyślnie.
- Branding firmy, kolorów i logo jest pobierany runtime przez `GET /api/branding`; obraz logo nie jest osadzany w `custom.txt` ani w paczce generatora.

### vs legacy CDAP Support Agent

Dawny Go/Wails Support Agent w monorepo BetterDesk został usunięty. Oficjalna ścieżka to **BetterDesk-Client** + `custom.txt` + Generator w panelu (`product_type: betterdesk-support`).

## Pliki kluczowe

| Temat | Ścieżka |
|-------|--------|
| Defaults / kill public | `libs/hbb_common/src/config.rs` |
| API / key / custom.txt | `src/common.rs` |
| BetterDesk HTTP (branding + enrollment) | `src/hbbs_http/betterdesk.rs` |
| Signing keys | `res/betterdesk/` |
| Scripts | `scripts/generate_custom_client_signing_key.py`, `sign_custom_client_config.py`, `pack_generator_templates.py` |
| Przykład full client | `examples/betterdesk-custom.example.json` |
| Przykład Support Agent | `examples/betterdesk-support-agent.example.json` |
| Clean desktop CI | `.github/workflows/betterdesk-desktop-release.yml` |
| Łączność | [CONNECTIVITY.md](CONNECTIVITY.md) |
