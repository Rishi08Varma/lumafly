# Lumafly setup

Lumafly runs entirely on your machines. The only cloud service it talks to is Gmail. AI runs on Ollama, either on the same machine or on a PC on your LAN.

## 1. Google Cloud project and OAuth client

You need your own OAuth client. It takes about ten minutes.

1. Go to https://console.cloud.google.com and sign in with any of your Google accounts.
2. Create a project: top bar project picker, **New project**, name it `Lumafly`, **Create**.
3. Enable the Gmail API: **APIs & Services** > **Library**, search `Gmail API`, **Enable**.
4. Configure the consent screen: **APIs & Services** > **OAuth consent screen** (Google now calls this **Google Auth Platform** > **Branding**).
   - User type: **External**.
   - App name `Lumafly`, your email as support email and developer contact. Save.
   - Under **Audience**, keep **Publishing status: Testing**.
   - Under **Test users**, add all three Gmail addresses you want Lumafly to manage.
   - Under **Data access** (scopes), add `https://www.googleapis.com/auth/gmail.modify` and `https://www.googleapis.com/auth/gmail.send`. Google will show a warning that these are sensitive scopes. That is fine for a Testing app with test users only.
5. Create the client: **APIs & Services** > **Credentials** > **Create credentials** > **OAuth client ID**.
   - Application type: **Desktop app**.
   - Name `Lumafly desktop`. **Create**.
   - Copy the **Client ID** and **Client secret**.
6. In Lumafly, open **Settings**, paste the Client ID and Client secret, **Save**. The secret is stored in your OS keychain, not on disk.
7. Click **Add Gmail account**. Your browser opens Google's sign-in. Pick one of the test-user addresses, accept the two Gmail permissions (Google shows an "unverified app" warning for Testing apps; click **Continue**), and return to Lumafly. Repeat for each account. The first sync fetches the last 30 days of each inbox and takes a minute or two per account.

### Why Testing status matters

An app in Testing status is limited to 100 test users and Google expires its refresh tokens after 7 days. Lumafly detects the resulting `invalid_grant` error and shows a **Re-authenticate** button next to the affected account. Click it, sign in again, and sync resumes. Publishing the app to Production would remove the 7 day limit but requires Google verification for the gmail scopes, which is not worth it for personal use.

## 2. Ollama

### On the PC that runs the model

1. Install Ollama from https://ollama.com/download (Windows installer, or the Linux script).
2. Pull the default model:

   ```
   ollama pull qwen3:30b-a3b
   ```

   It is about 19 GB. Any other Ollama model works too. Set its name in Lumafly settings.
3. Let other machines reach it. By default Ollama listens only on 127.0.0.1.

   **Windows**: quit Ollama from the tray icon. Open **Settings** > **System** > **About** > **Advanced system settings** > **Environment Variables**. Under *User variables* add `OLLAMA_HOST` with value `0.0.0.0`. Start Ollama again from the Start menu. When Windows Firewall asks, allow it on private networks. If it does not ask, add an inbound rule for TCP port 11434.

   **Linux (systemd)**:

   ```
   sudo systemctl edit ollama
   ```

   Add:

   ```
   [Service]
   Environment="OLLAMA_HOST=0.0.0.0"
   ```

   Then `sudo systemctl restart ollama`.

4. Find the PC's LAN IP (`ipconfig` on Windows, `ip a` on Linux), for example `192.168.1.50`.
5. Verify from the PC itself:

   ```
   curl http://localhost:11434/api/version
   ```

### On the Mac (or any machine that only uses the PC)

In Lumafly **Settings** > **Ollama**, set Base URL to `http://192.168.1.50:11434` (your PC's IP), keep the model name, click **Test connection**. Lumafly will not try to start or stop Ollama when the URL is not localhost.

### Running the model on the same machine as Lumafly

Install Ollama and pull the model as above. Leave Base URL as `http://localhost:11434`. On launch Lumafly checks whether Ollama is responding; if not it starts `ollama serve` itself and stops it again when you quit. It never stops an Ollama instance it did not start. Every request uses `keep_alive: 5m`, so the model leaves RAM after five idle minutes.

## 3. Running Lumafly from source

Prerequisites: Node 20+, Rust stable (https://rustup.rs), and the Tauri platform deps:

- macOS: Xcode Command Line Tools (`xcode-select --install`).
- Windows: Visual Studio Build Tools with the C++ workload, and WebView2 (already on Windows 11).
- Linux (Debian/Ubuntu): `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev`

Then:

```
npm install
npm run tauri dev
```

## 3b. Building installers

Each OS builds its own installer. Cross-compiling is not supported by Tauri, so use the GitHub Actions workflow in `.github/workflows/release.yml` to get all three from one push, or build locally on each machine.

Local build, on the machine whose installer you want:

```
npm run tauri build
```

Outputs land in `src-tauri/target/release/bundle/`:

- macOS: `dmg/Lumafly_0.1.1_aarch64.dmg` (or `_x64` on Intel) and `macos/Lumafly.app`
- Windows: `msi/Lumafly_0.1.1_x64_en-US.msi`
- Linux: `appimage/Lumafly_0.1.1_amd64.AppImage` and `deb/Lumafly_0.1.1_amd64.deb`

To build only one format, pass `--bundles`, for example `npm run tauri build -- --bundles msi`.

Linux builds additionally need `libdbus-1-dev` (for the keyring) and `patchelf` (for AppImage) on top of the deps listed above.

### GitHub release

Push the repo to GitHub, then tag a version:

```
git tag v0.1.1 && git push origin v0.1.1
```

The workflow builds macOS (Apple Silicon and Intel), Windows, and Linux in parallel and attaches the installers to a draft release. Open the release on GitHub, check the files, and click Publish. You can also run it by hand from the Actions tab with "Run workflow".

### Unsigned builds

Nothing is code-signed. Each OS will warn once:

- **macOS**: a downloaded Lumafly.app shows "is damaged and can't be opened". That is Gatekeeper's message for unsigned downloads. Fix it once after copying to Applications:

  ```
  xattr -cr /Applications/Lumafly.app
  ```

  An app built locally on the same Mac does not need this.
- **Windows**: SmartScreen says "Windows protected your PC". Click More info, then Run anyway.
- **Linux**: mark the AppImage executable (`chmod +x`) or install the .deb with `sudo apt install ./Lumafly_0.1.1_amd64.deb`.

Signing certificates (Apple Developer, Windows EV) would remove these warnings but cost money and are not needed for personal use.

## 4. Where data lives

- Settings: the OS app config directory (`~/Library/Application Support/app.lumafly.desktop` on macOS, `%APPDATA%\app.lumafly.desktop` on Windows, `~/.config/app.lumafly.desktop` on Linux), file `settings.json`.
- Database: same directory, `lumafly.db` (SQLite).
- OAuth tokens and the client secret: OS keychain (Keychain Access, Windows Credential Manager, or the Secret Service on Linux).

Lumafly never permanently deletes mail. It only trashes, marks spam, archives, or labels. Gmail empties Trash and Spam after 30 days on its own.
