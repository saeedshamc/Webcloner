const { invoke } = window.__TAURI__.tauri;

const urlEl = document.getElementById("url");
const outEl = document.getElementById("out");
const maxPagesEl = document.getElementById("maxPages");
const maxDepthEl = document.getElementById("maxDepth");
const concurrencyEl = document.getElementById("concurrency");
const externalAssetsEl = document.getElementById("externalAssets");
const followExternalEl = document.getElementById("followExternal");
const zipEl = document.getElementById("zip");
const downloadBtn = document.getElementById("downloadBtn");
const openBtn = document.getElementById("openBtn");
const logEl = document.getElementById("log");

let lastOutDir = null;

function appendLog(text, kind = "") {
  const line = document.createElement("div");
  if (kind) line.className = kind;
  line.textContent = text;
  logEl.appendChild(line);
  logEl.scrollTop = logEl.scrollHeight;
}

function setBusy(busy) {
  downloadBtn.disabled = busy;
  openBtn.disabled = busy || !lastOutDir;
}

downloadBtn.addEventListener("click", async () => {
  const url = urlEl.value.trim();
  if (!url) {
    appendLog("لطفاً آدرس سایت را وارد کنید.", "err");
    return;
  }

  logEl.textContent = "";
  setBusy(true);
  appendLog("در حال دانلود...");

  try {
    const result = await invoke("download_site", {
      options: {
        url,
        out: outEl.value.trim() || "cloned-site",
        maxPages: Number(maxPagesEl.value) || 40,
        maxDepth: Number(maxDepthEl.value) || 3,
        concurrency: Number(concurrencyEl.value) || 8,
        includeExternalAssets: externalAssetsEl.checked,
        followExternalPages: followExternalEl.checked,
        zip: zipEl.checked,
      },
    });
    lastOutDir = result.outDir;
    appendLog(result.message, "ok");
    openBtn.disabled = false;
  } catch (err) {
    appendLog(String(err), "err");
  } finally {
    setBusy(false);
  }
});

openBtn.addEventListener("click", async () => {
  if (!lastOutDir) return;
  try {
    await invoke("open_folder", { path: lastOutDir });
  } catch (err) {
    appendLog(String(err), "err");
  }
});
