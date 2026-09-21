const { invoke } = window.__TAURI__.tauri;
const { listen } = window.__TAURI__.event;

const urlEl = document.getElementById("url");
const saveDirEl = document.getElementById("saveDir");
const outNameEl = document.getElementById("outName");
const pickDirBtn = document.getElementById("pickDirBtn");
const maxPagesEl = document.getElementById("maxPages");
const maxDepthEl = document.getElementById("maxDepth");
const concurrencyEl = document.getElementById("concurrency");
const externalAssetsEl = document.getElementById("externalAssets");
const followExternalEl = document.getElementById("followExternal");
const blockTrackingEl = document.getElementById("blockTracking");
const reportBrokenEl = document.getElementById("reportBroken");
const zipEl = document.getElementById("zip");
const cloneProfileEl = document.getElementById("cloneProfile");
const downloadBtn = document.getElementById("downloadBtn");
const discoverBtn = document.getElementById("discoverBtn");
const resumeBtn = document.getElementById("resumeBtn");
const cancelDownloadBtn = document.getElementById("cancelDownloadBtn");
const openBtn = document.getElementById("openBtn");
const useForServerBtn = document.getElementById("useForServerBtn");
const desktopPkgBtn = document.getElementById("desktopPkgBtn");
const logEl = document.getElementById("log");
const downloadBadge = document.getElementById("downloadBadge");
const progressWrap = document.getElementById("progressWrap");
const progressFill = document.getElementById("progressFill");
const progressLabel = document.getElementById("progressLabel");
const recentClones = document.getElementById("recentClones");
const recentClonesList = document.getElementById("recentClonesList");
const discoverBox = document.getElementById("discoverBox");
const discoverList = document.getElementById("discoverList");
const discoverSummary = document.getElementById("discoverSummary");
const selectAllUrlsBtn = document.getElementById("selectAllUrlsBtn");
const clearUrlsBtn = document.getElementById("clearUrlsBtn");
const confirmDiscoverBtn = document.getElementById("confirmDiscoverBtn");
const resumeHint = document.getElementById("resumeHint");

const projectDirEl = document.getElementById("projectDir");
const pickProjectBtn = document.getElementById("pickProjectBtn");
const projectScanEl = document.getElementById("projectScan");
const scanTagsEl = document.getElementById("scanTags");
const serverBackendEl = document.getElementById("serverBackend");
const serverPortEl = document.getElementById("serverPort");
const autoPortEl = document.getElementById("autoPort");
const backendNoteEl = document.getElementById("backendNote");
const serverStatusTextEl = document.getElementById("serverStatusText");
const serverUrlBoxEl = document.getElementById("serverUrlBox");
const serverUrlEl = document.getElementById("serverUrl");
const startServerBtn = document.getElementById("startServerBtn");
const stopServerBtn = document.getElementById("stopServerBtn");
const previewBtn = document.getElementById("previewBtn");
const openSiteBtn = document.getElementById("openSiteBtn");
const openProjectBtn = document.getElementById("openProjectBtn");
const serverLogEl = document.getElementById("serverLog");
const serverBadge = document.getElementById("serverBadge");
const recentServers = document.getElementById("recentServers");
const recentServersList = document.getElementById("recentServersList");
const runtimeStatusText = document.getElementById("runtimeStatusText");

const LOG_MAX_LINES = 400;
const PROFILES = {
  fast: { maxPages: 10, maxDepth: 1, concurrency: 6, followExternal: false, externalAssets: true },
  full: { maxPages: 80, maxDepth: 4, concurrency: 10, followExternal: false, externalAssets: true },
  assetsOnly: { maxPages: 1, maxDepth: 0, concurrency: 8, followExternal: false, externalAssets: true },
};

let lastOutDir = null;
let activeServerUrl = null;
let currentScan = null;
let downloadBusy = false;
let serverBusy = false;
let serverRunning = false;
let settingsCache = null;
let discoveredPages = [];

function appendLog(target, text, kind = "") {
  const line = document.createElement("div");
  if (kind) line.className = kind;
  line.textContent = text;
  target.appendChild(line);
  while (target.childElementCount > LOG_MAX_LINES) {
    target.removeChild(target.firstElementChild);
  }
  target.scrollTop = target.scrollHeight;
}

function normalizeUrlInput(raw) {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  return `https://${trimmed.replace(/^\/+/, "")}`;
}

function parseServerPort() {
  const raw = Number(serverPortEl.value);
  if (!Number.isFinite(raw) || !Number.isInteger(raw) || raw < 1024 || raw > 65535) {
    return null;
  }
  return raw;
}

function setProgress(pct, label) {
  const value = Math.max(0, Math.min(100, Math.round(pct)));
  progressWrap.classList.remove("hidden");
  progressFill.style.width = `${value}%`;
  progressLabel.textContent = label || `${value}%`;
}

function hideProgress() {
  progressWrap.classList.add("hidden");
  progressFill.style.width = "0%";
}

async function expectedOutputPath() {
  const saveDir = saveDirEl.value.trim();
  const outName = outNameEl.value.trim() || "cloned-site";
  if (!saveDir) return null;
  try {
    return await invoke("resolve_clone_output_path", { saveDir, outName });
  } catch {
    return null;
  }
}

function setDownloadBusy(busy) {
  downloadBusy = busy;
  downloadBtn.disabled = busy;
  discoverBtn.disabled = busy;
  resumeBtn.disabled = busy || resumeBtn.dataset.available !== "1";
  cancelDownloadBtn.disabled = !busy;
  pickDirBtn.disabled = busy;
  cloneProfileEl.disabled = busy;
  confirmDiscoverBtn.disabled = busy;
  openBtn.disabled = busy || !lastOutDir;
  desktopPkgBtn.disabled = busy || !lastOutDir;
  useForServerBtn.disabled = !saveDirEl.value.trim();
  downloadBadge.classList.toggle("hidden", !busy);
  downloadBadge.classList.toggle("downloading", busy);
  if (!busy) hideProgress();
}

function renderDiscoverList(pages) {
  discoveredPages = pages || [];
  discoverList.innerHTML = "";
  discoveredPages.forEach((page, idx) => {
    const label = document.createElement("label");
    const cb = document.createElement("input");
    cb.type = "checkbox";
    cb.checked = true;
    cb.dataset.idx = String(idx);
    const span = document.createElement("span");
    span.textContent = `[${page.depth}] ${page.url}`;
    label.appendChild(cb);
    label.appendChild(span);
    discoverList.appendChild(label);
  });
  discoverBox.classList.toggle("hidden", discoveredPages.length === 0);
  discoverSummary.textContent = `${discoveredPages.length} آدرس پیدا شد — موارد دلخواه را تأیید کنید`;
}

function selectedDiscoverUrls() {
  return Array.from(discoverList.querySelectorAll("input[type=checkbox]:checked")).map((cb) => {
    const idx = Number(cb.dataset.idx);
    return discoveredPages[idx]?.url;
  }).filter(Boolean);
}

async function refreshResumeHint() {
  const saveDir = saveDirEl.value.trim();
  const outName = outNameEl.value.trim() || "cloned-site";
  if (!saveDir) {
    resumeBtn.disabled = true;
    resumeBtn.dataset.available = "0";
    resumeHint.classList.add("hidden");
    return;
  }
  try {
    const info = await invoke("check_resume_job", { saveDir, outName });
    resumeBtn.dataset.available = info.available ? "1" : "0";
    resumeBtn.disabled = downloadBusy || !info.available;
    if (info.available) {
      resumeHint.textContent = info.message;
      resumeHint.classList.remove("hidden");
      if (info.startUrl && !urlEl.value.trim()) urlEl.value = info.startUrl;
    } else {
      resumeHint.classList.add("hidden");
    }
  } catch {
    resumeBtn.disabled = true;
    resumeBtn.dataset.available = "0";
  }
}

function buildDownloadOptions(extra = {}) {
  return {
    url: normalizeUrlInput(urlEl.value) || "https://example.com",
    saveDir: saveDirEl.value.trim(),
    outName: outNameEl.value.trim() || "cloned-site",
    maxPages: Number(maxPagesEl.value) || 40,
    maxDepth: Number(maxDepthEl.value) || 3,
    concurrency: Number(concurrencyEl.value) || 8,
    includeExternalAssets: externalAssetsEl.checked,
    followExternalPages: followExternalEl.checked,
    zip: zipEl.checked,
    blockTracking: blockTrackingEl.checked,
    reportBrokenLinks: reportBrokenEl.checked,
    plannedPages: null,
    resume: false,
    ...extra,
  };
}

function updateServerControls() {
  const controlsLocked = serverRunning || serverBusy;
  serverStatusTextEl.classList.toggle("running", serverRunning);
  startServerBtn.disabled = controlsLocked;
  stopServerBtn.disabled = !serverRunning || serverBusy;
  previewBtn.disabled = !serverRunning;
  openSiteBtn.disabled = !serverRunning;
  pickProjectBtn.disabled = serverBusy;
  serverBackendEl.disabled = controlsLocked;
  serverPortEl.disabled = controlsLocked;
  autoPortEl.disabled = controlsLocked;
  openProjectBtn.disabled = !projectDirEl.value.trim();
  serverBadge.classList.toggle("hidden", !serverRunning);
}

function backendLabel(backend) {
  if (backend === "static") return "استاتیک";
  if (backend === "php") return "PHP";
  if (backend === "aspNet") return "ASP.NET";
  return backend;
}

function renderScan(scan) {
  currentScan = scan;
  projectScanEl.classList.remove("hidden");
  const tags = [];
  if (scan.hasHtml) tags.push("HTML");
  if (scan.hasPhp) tags.push("PHP");
  if (scan.hasAsp) tags.push("ASP");
  if (scan.hasAspx) tags.push("ASPX");
  if (scan.hasCsproj) tags.push(".NET");
  scanTagsEl.textContent = tags.length
    ? tags.join(" · ")
    : "فایل شناخته‌شده‌ای پیدا نشد — حالت استاتیک پیشنهاد می‌شود";
  serverBackendEl.innerHTML = "";
  scan.backends.forEach((item) => {
    const option = document.createElement("option");
    option.value = item.backend;
    option.textContent = item.available ? item.label : `${item.label} (غیرفعال)`;
    option.disabled = !item.available;
    if (item.backend === scan.recommended && item.available) option.selected = true;
    serverBackendEl.appendChild(option);
  });
  updateBackendNote();
}

function updateBackendNote() {
  if (!currentScan) {
    backendNoteEl.textContent = "";
    return;
  }
  const item = currentScan.backends.find((b) => b.backend === serverBackendEl.value);
  backendNoteEl.textContent = item ? item.note : "";
}

function updateServerUi(status) {
  activeServerUrl = status.running ? status.url : null;
  serverRunning = Boolean(status.running);
  serverBusy = Boolean(status.busy);
  serverStatusTextEl.textContent = serverRunning
    ? `در حال اجرا روی پورت ${status.port ?? "?"} (${backendLabel(status.backend || "static")})`
    : serverBusy
      ? "در حال پردازش..."
      : "متوقف";
  if (serverRunning && status.port) serverPortEl.value = String(status.port);
  if (serverRunning && status.url) {
    serverUrlBoxEl.classList.remove("hidden");
    serverUrlEl.textContent = status.url;
    serverUrlEl.href = status.url;
  } else {
    serverUrlBoxEl.classList.add("hidden");
    serverUrlEl.textContent = "";
    serverUrlEl.href = "#";
  }
  updateServerControls();
}

function renderRecent(settings) {
  const clones = (settings.recent || []).filter((r) => r.kind === "clone");
  const servers = (settings.recent || []).filter((r) => r.kind === "server");
  recentClones.classList.toggle("hidden", clones.length === 0);
  recentServers.classList.toggle("hidden", servers.length === 0);
  recentClonesList.innerHTML = "";
  recentServersList.innerHTML = "";
  clones.slice(0, 5).forEach((item) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "recent-item";
    btn.textContent = item.path.split(/[/\\]/).pop() || item.path;
    btn.title = item.path;
    btn.addEventListener("click", async () => {
      lastOutDir = item.path;
      if (item.url) urlEl.value = item.url;
      projectDirEl.value = item.path;
      openBtn.disabled = false;
      desktopPkgBtn.disabled = false;
      await scanProject(item.path);
      appendLog(logEl, `انتخاب از تاریخچه: ${item.path}`);
    });
    recentClonesList.appendChild(btn);
  });
  servers.slice(0, 5).forEach((item) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "recent-item";
    btn.textContent = `${item.path.split(/[/\\]/).pop() || item.path}${item.port ? ` :${item.port}` : ""}`;
    btn.title = item.path;
    btn.addEventListener("click", async () => {
      projectDirEl.value = item.path;
      if (item.port) serverPortEl.value = String(item.port);
      await scanProject(item.path);
      appendLog(serverLogEl, `انتخاب از تاریخچه: ${item.path}`);
    });
    recentServersList.appendChild(btn);
  });
}

async function persistSettingsPartial(partial) {
  const base = settingsCache || (await invoke("load_app_settings"));
  settingsCache = { ...base, ...partial, recent: base.recent || [] };
  await invoke("save_app_settings", { settings: settingsCache });
  renderRecent(settingsCache);
}

async function refreshServerStatus() {
  try {
    const status = await invoke("get_local_server_status");
    updateServerUi(status);
    if (status.projectDir && !projectDirEl.value) {
      projectDirEl.value = status.projectDir;
      await scanProject(status.projectDir);
    }
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
}

async function scanProject(dir) {
  if (!dir) return;
  try {
    const scan = await invoke("scan_local_project", { projectDir: dir });
    renderScan(scan);
  } catch (err) {
    projectScanEl.classList.add("hidden");
    appendLog(serverLogEl, String(err), "err");
  }
}

async function applyOutputPathToServer() {
  const path = lastOutDir || (await expectedOutputPath());
  if (!path) {
    appendLog(serverLogEl, "ابتدا محل ذخیره و نام پوشه را مشخص کنید.", "err");
    return;
  }
  projectDirEl.value = path;
  serverLogEl.textContent = "";
  appendLog(serverLogEl, `پوشه سرور: ${path}`);
  await scanProject(path);
}

async function refreshRuntimeStatus() {
  try {
    const st = await invoke("get_runtime_status");
    const php = st.phpAvailable
      ? `PHP: ${st.phpBundled ? "داخلی" : "سیستم"}`
      : "PHP: نیست";
    const dot = st.dotnetAvailable
      ? `.NET: ${st.dotnetBundled ? "داخلی" : "سیستم"}`
      : ".NET: نیست";
    runtimeStatusText.textContent = `${php} · ${dot}`;
  } catch (err) {
    runtimeStatusText.textContent = String(err);
  }
}

async function initSettings() {
  try {
    const settings = await invoke("load_app_settings");
    settingsCache = settings;
    if (settings.saveDir) saveDirEl.value = settings.saveDir;
    else await initSaveDir();
    if (settings.outName) outNameEl.value = settings.outName;
    if (settings.maxPages) maxPagesEl.value = settings.maxPages;
    if (settings.maxDepth != null) maxDepthEl.value = settings.maxDepth;
    if (settings.concurrency) concurrencyEl.value = settings.concurrency;
    if (settings.includeExternalAssets != null) externalAssetsEl.checked = settings.includeExternalAssets;
    if (settings.followExternalPages != null) followExternalEl.checked = settings.followExternalPages;
    if (settings.zip != null) zipEl.checked = settings.zip;
    if (settings.blockTracking != null) blockTrackingEl.checked = settings.blockTracking;
    if (settings.reportBrokenLinks != null) reportBrokenEl.checked = settings.reportBrokenLinks;
    if (settings.port) serverPortEl.value = settings.port;
    if (settings.autoPort != null) autoPortEl.checked = settings.autoPort;
    renderRecent(settings);
  } catch (err) {
    appendLog(logEl, `خطا در تنظیمات: ${err}`, "err");
    await initSaveDir();
  }
}

async function initSaveDir() {
  try {
    const defaultDir = await invoke("get_default_save_dir");
    if (defaultDir && !saveDirEl.value) saveDirEl.value = defaultDir;
  } catch (err) {
    appendLog(logEl, `خطا در مسیر پیش‌فرض: ${err}`, "err");
  }
}

cloneProfileEl.addEventListener("change", () => {
  const key = cloneProfileEl.value;
  const profile = PROFILES[key];
  if (!profile) return;
  maxPagesEl.value = profile.maxPages;
  maxDepthEl.value = profile.maxDepth;
  concurrencyEl.value = profile.concurrency;
  followExternalEl.checked = profile.followExternal;
  externalAssetsEl.checked = profile.externalAssets;
});

pickDirBtn.addEventListener("click", async () => {
  try {
    const picked = await invoke("pick_output_folder");
    if (picked) {
      saveDirEl.value = picked;
      await persistSettingsPartial({ saveDir: picked });
    }
    useForServerBtn.disabled = !saveDirEl.value.trim();
  } catch (err) {
    appendLog(logEl, String(err), "err");
  }
});

pickProjectBtn.addEventListener("click", async () => {
  try {
    const picked = await invoke("pick_project_folder");
    if (picked) {
      projectDirEl.value = picked;
      serverLogEl.textContent = "";
      await scanProject(picked);
    }
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
});

serverBackendEl.addEventListener("change", updateBackendNote);

downloadBtn.addEventListener("click", async () => {
  const url = normalizeUrlInput(urlEl.value);
  if (!url) {
    appendLog(logEl, "لطفاً آدرس سایت را وارد کنید.", "err");
    return;
  }
  if (!saveDirEl.value.trim()) await initSaveDir();
  const saveDir = saveDirEl.value.trim();
  if (!saveDir) {
    appendLog(logEl, "لطفاً محل ذخیره‌سازی را انتخاب کنید.", "err");
    return;
  }
  urlEl.value = url;
  logEl.textContent = "";
  setDownloadBusy(true);
  setProgress(2, "شروع...");
  appendLog(logEl, `در حال دانلود از ${url} ...`);

  try {
    const result = await invoke("download_site", {
      options: buildDownloadOptions({ url, saveDir, plannedPages: null, resume: false }),
    });
    lastOutDir = result.outDir;
    appendLog(logEl, result.message, result.cancelled ? "err" : "ok");
    openBtn.disabled = false;
    desktopPkgBtn.disabled = false;
    settingsCache = await invoke("load_app_settings");
    renderRecent(settingsCache);
    await refreshResumeHint();
    if (!result.cancelled) setProgress(100, "تمام");
  } catch (err) {
    appendLog(logEl, String(err), "err");
    await refreshResumeHint();
  } finally {
    setDownloadBusy(false);
  }
});

discoverBtn.addEventListener("click", async () => {
  const url = normalizeUrlInput(urlEl.value);
  if (!url) {
    appendLog(logEl, "لطفاً آدرس سایت را وارد کنید.", "err");
    return;
  }
  urlEl.value = url;
  logEl.textContent = "";
  setDownloadBusy(true);
  setProgress(5, "کشف آدرس‌ها...");
  appendLog(logEl, `در حال کشف همه صفحات از ${url} ...`);
  try {
    const result = await invoke("discover_site", {
      options: {
        url,
        maxPagesCap: 2000,
        maxDepth: Math.max(Number(maxDepthEl.value) || 3, 20),
        followExternalPages: followExternalEl.checked,
        blockTracking: blockTrackingEl.checked,
      },
    });
    appendLog(logEl, result.message, "ok");
    renderDiscoverList(result.pages || []);
    setProgress(100, "کشف تمام شد");
  } catch (err) {
    appendLog(logEl, String(err), "err");
  } finally {
    setDownloadBusy(false);
  }
});

selectAllUrlsBtn.addEventListener("click", () => {
  discoverList.querySelectorAll("input[type=checkbox]").forEach((cb) => {
    cb.checked = true;
  });
});

clearUrlsBtn.addEventListener("click", () => {
  discoverList.querySelectorAll("input[type=checkbox]").forEach((cb) => {
    cb.checked = false;
  });
});

confirmDiscoverBtn.addEventListener("click", async () => {
  const urls = selectedDiscoverUrls();
  if (!urls.length) {
    appendLog(logEl, "حداقل یک آدرس را انتخاب کنید.", "err");
    return;
  }
  if (!saveDirEl.value.trim()) await initSaveDir();
  const saveDir = saveDirEl.value.trim();
  if (!saveDir) {
    appendLog(logEl, "لطفاً محل ذخیره‌سازی را انتخاب کنید.", "err");
    return;
  }
  const url = normalizeUrlInput(urlEl.value) || urls[0];
  urlEl.value = url;
  logEl.textContent = "";
  setDownloadBusy(true);
  setProgress(2, "شروع دانلود تأییدشده...");
  appendLog(logEl, `دانلود ${urls.length} آدرس تأییدشده...`);
  try {
    const result = await invoke("download_site", {
      options: buildDownloadOptions({
        url,
        saveDir,
        maxPages: urls.length,
        plannedPages: urls,
        resume: false,
      }),
    });
    lastOutDir = result.outDir;
    appendLog(logEl, result.message, result.cancelled ? "err" : "ok");
    openBtn.disabled = false;
    desktopPkgBtn.disabled = false;
    settingsCache = await invoke("load_app_settings");
    renderRecent(settingsCache);
    await refreshResumeHint();
    if (!result.cancelled) {
      setProgress(100, "تمام");
      discoverBox.classList.add("hidden");
    }
  } catch (err) {
    appendLog(logEl, String(err), "err");
    await refreshResumeHint();
  } finally {
    setDownloadBusy(false);
  }
});

resumeBtn.addEventListener("click", async () => {
  if (!saveDirEl.value.trim()) await initSaveDir();
  const saveDir = saveDirEl.value.trim();
  if (!saveDir) {
    appendLog(logEl, "لطفاً محل ذخیره‌سازی را انتخاب کنید.", "err");
    return;
  }
  logEl.textContent = "";
  setDownloadBusy(true);
  setProgress(5, "ادامه دانلود...");
  appendLog(logEl, "ادامه از فایل وضعیت ذخیره‌شده...");
  try {
    const result = await invoke("download_site", {
      options: buildDownloadOptions({
        url: normalizeUrlInput(urlEl.value) || "https://example.com",
        saveDir,
        resume: true,
        plannedPages: null,
      }),
    });
    lastOutDir = result.outDir;
    appendLog(logEl, result.message, result.cancelled ? "err" : "ok");
    openBtn.disabled = false;
    desktopPkgBtn.disabled = false;
    settingsCache = await invoke("load_app_settings");
    renderRecent(settingsCache);
    await refreshResumeHint();
    if (!result.cancelled) setProgress(100, "تمام");
  } catch (err) {
    appendLog(logEl, String(err), "err");
    await refreshResumeHint();
  } finally {
    setDownloadBusy(false);
  }
});

cancelDownloadBtn.addEventListener("click", async () => {
  try {
    await invoke("cancel_download");
    appendLog(logEl, "درخواست توقف ارسال شد — پیشرفت ذخیره می‌شود...", "err");
  } catch (err) {
    appendLog(logEl, String(err), "err");
  }
});

openBtn.addEventListener("click", async () => {
  if (!lastOutDir) return;
  try {
    await invoke("open_folder", { path: lastOutDir });
  } catch (err) {
    appendLog(logEl, String(err), "err");
  }
});

useForServerBtn.addEventListener("click", applyOutputPathToServer);

desktopPkgBtn.addEventListener("click", async () => {
  if (!lastOutDir) return;
  appendLog(logEl, "در حال آماده‌سازی بسته دسکتاپ...");
  try {
    const result = await invoke("prepare_desktop_package", { projectDir: lastOutDir });
    appendLog(logEl, result.message, "ok");
    await invoke("open_folder", { path: result.outputDir });
  } catch (err) {
    appendLog(logEl, String(err), "err");
  }
});

startServerBtn.addEventListener("click", async () => {
  let projectDir = projectDirEl.value.trim();
  if (!projectDir) {
    const predicted = await expectedOutputPath();
    if (predicted) {
      projectDir = predicted;
      projectDirEl.value = predicted;
    }
  }
  if (!projectDir) {
    appendLog(serverLogEl, "لطفاً پوشه پروژه را انتخاب کنید.", "err");
    return;
  }
  let port = parseServerPort();
  if (port === null) {
    appendLog(serverLogEl, "پورت باید عددی بین 1024 تا 65535 باشد.", "err");
    return;
  }
  const autoPort = autoPortEl.checked;
  if (!autoPort) {
    try {
      const free = await invoke("suggest_free_port", { preferred: port });
      if (free !== port) {
        appendLog(
          serverLogEl,
          `پورت ${port} ممکن است اشغال باشد. آزاد پیشنهادی: ${free}`,
          "err"
        );
      }
    } catch (_) {}
  }
  const backend = serverBackendEl.value;
  serverBusy = true;
  updateServerControls();
  appendLog(serverLogEl, `در حال راه‌اندازی سرور روی پورت ${port}...`);
  try {
    const result = await invoke("start_local_server", {
      options: { projectDir, port, backend, autoPort },
    });
    serverPortEl.value = String(result.port);
    appendLog(serverLogEl, result.message, "ok");
    await scanProject(projectDir);
    settingsCache = await invoke("load_app_settings");
    renderRecent(settingsCache);
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  } finally {
    await refreshServerStatus();
  }
});

stopServerBtn.addEventListener("click", async () => {
  serverBusy = true;
  updateServerControls();
  appendLog(serverLogEl, "در حال توقف سرور...");
  try {
    await invoke("stop_local_server");
    appendLog(serverLogEl, "سرور متوقف شد.", "ok");
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  } finally {
    await refreshServerStatus();
  }
});

previewBtn.addEventListener("click", async () => {
  if (!activeServerUrl) return;
  try {
    await invoke("open_preview_window", { url: activeServerUrl });
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
});

openSiteBtn.addEventListener("click", async () => {
  if (!activeServerUrl) return;
  try {
    await invoke("open_url", { url: activeServerUrl });
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
});

openProjectBtn.addEventListener("click", async () => {
  const dir = projectDirEl.value.trim();
  if (!dir) return;
  try {
    await invoke("open_folder", { path: dir });
  } catch (err) {
    appendLog(serverLogEl, String(err), "err");
  }
});

initSettings().then(() => refreshResumeHint());
refreshServerStatus();
refreshRuntimeStatus();
setInterval(refreshServerStatus, 4000);
outNameEl.addEventListener("change", refreshResumeHint);
saveDirEl.addEventListener("change", refreshResumeHint);

listen("download-progress", (event) => {
  if (typeof event.payload === "string") appendLog(logEl, event.payload);
});

listen("download-progress-event", (event) => {
  const ev = event.payload;
  if (!ev || typeof ev !== "object") return;
  const pagesTotal = Math.max(1, ev.pagesTotal || 1);
  const assetsKnown = Math.max(1, ev.assetsKnown || 1);
  let pct = 5;
  if (ev.phase === "pages") pct = 10 + (40 * (ev.pagesDone || 0)) / pagesTotal;
  else if (ev.phase === "assets") pct = 50 + (40 * (ev.assetsDone || 0)) / assetsKnown;
  else if (ev.phase === "rewrite") pct = 92;
  else if (ev.phase === "done") pct = 100;
  else if (ev.phase === "cancelled") pct = Math.max(10, (ev.pagesDone || 0));
  setProgress(pct, ev.message || ev.phase);
});
