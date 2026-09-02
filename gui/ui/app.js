const { invoke } = window.__TAURI__.tauri;

const urlEl = document.getElementById("url");
const saveDirEl = document.getElementById("saveDir");
const outNameEl = document.getElementById("outName");
const pickDirBtn = document.getElementById("pickDirBtn");
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
  pickDirBtn.disabled = busy;
  openBtn.disabled = busy || !lastOutDir;
}

function normalizeUrlInput(raw) {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  return `https://${trimmed.replace(/^\/+/, "")}`;
}

async function initSaveDir() {
  try {
    const defaultDir = await invoke("get_default_save_dir");
    if (defaultDir && !saveDirEl.value) {
      saveDirEl.value = defaultDir;
    }
  } catch (err) {
    appendLog(`خطا در خواندن مسیر پیش‌فرض: ${err}`, "err");
  }
}

pickDirBtn.addEventListener("click", async () => {
  try {
    const picked = await invoke("pick_output_folder");
    if (picked) {
      saveDirEl.value = picked;
    }
  } catch (err) {
    appendLog(String(err), "err");
  }
});

downloadBtn.addEventListener("click", async () => {
  const url = normalizeUrlInput(urlEl.value);
  if (!url) {
    appendLog("لطفاً آدرس سایت را وارد کنید.", "err");
    return;
  }

  const saveDir = saveDirEl.value.trim();
  if (!saveDir) {
    appendLog("لطفاً محل ذخیره‌سازی را انتخاب کنید.", "err");
    return;
  }

  const outName = outNameEl.value.trim() || "cloned-site";
  urlEl.value = url;

  logEl.textContent = "";
  setBusy(true);
  appendLog(`در حال دانلود از ${url} ...`);

  try {
    const result = await invoke("download_site", {
      options: {
        url,
        saveDir,
        outName,
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

initSaveDir();
