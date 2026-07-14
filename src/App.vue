<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, watch } from "vue";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { UnlistenFn } from "@tauri-apps/api/event";

// Types
interface BookInfo {
  bookId: string;
  bookSlug: string;
  title: string;
  totalPages: number;
  pages: string[];
  pageIds: string[];
  author: string | null;
  publisher: string | null;
  year: string | null;
  coverUrl: string | null;
}

interface DownloadProgress {
  completed: number;
  total: number;
  percentage: number;
  message: string;
  phase: string;
}

// State
const bookUrl = ref("");
const currentBookInfo = ref<BookInfo | null>(null);
const isLoading = ref(false);
const isDownloading = ref(false);
const showPreview = ref(false);
const logs = ref<string[]>([]);

const downloadProgress = ref<DownloadProgress | null>(null);
const currentMessageIndex = ref(0);
const showAnimatedMessages = ref(false);
const resultMessage = ref("");
const resultType = ref<"success" | "error" | "">("");

// Animated messages for PDF creation phase
const pdfCreationMessages = [
  "Reconstructing pages from tiles...",
  "Assembling your digital book...",
  "Adding the finishing touches...",
  "Putting all pages together...",
  "Optimizing image quality...",
  "Creating the perfect PDF...",
  "Almost there, just a few moments...",
  "Fine-tuning the final document...",
  "Your book is almost ready...",
];

// Computed
const progressPercentage = computed(() => {
  if (!downloadProgress.value) return 0;
  return Math.round(downloadProgress.value.percentage);
});

const isPdfPhase = computed(() => {
  return downloadProgress.value?.phase === "creating_pdf";
});

// Message rotation
let messageInterval: ReturnType<typeof setInterval> | null = null;

watch(isPdfPhase, (val) => {
  if (val) {
    showAnimatedMessages.value = true;
    currentMessageIndex.value = 0;
    messageInterval = setInterval(() => {
      currentMessageIndex.value =
        (currentMessageIndex.value + 1) % pdfCreationMessages.length;
    }, 2000);
  } else {
    showAnimatedMessages.value = false;
    if (messageInterval) {
      clearInterval(messageInterval);
      messageInterval = null;
    }
  }
});

// Event listener
let unlisten: UnlistenFn | null = null;

onMounted(async () => {
  unlisten = await listen<DownloadProgress>("download-progress", (event) => {
    downloadProgress.value = event.payload;
    addLog(event.payload.message);
  });
});

onUnmounted(() => {
  if (unlisten) unlisten();
  if (messageInterval) clearInterval(messageInterval);
});

// Methods
function addLog(message: string) {
  logs.value.push(message);
}

async function previewBook() {
  const url = bookUrl.value.trim();
  if (!url || !url.includes("rekhta.org/ebooks/")) {
    resultMessage.value = "Please enter a valid Rekhta ebook URL";
    resultType.value = "error";
    return;
  }

  isLoading.value = true;
  showPreview.value = false;
  resultMessage.value = "";
  resultType.value = "";
  logs.value = [];

  try {
    addLog("Fetching book information...");
    const bookInfo = await invoke<BookInfo>("fetch_book_info", { url });
    currentBookInfo.value = bookInfo;
    showPreview.value = true;
    addLog(
      `Book preview loaded: ${bookInfo.title} (${bookInfo.totalPages} pages)`
    );
  } catch (error) {
    resultMessage.value = `Error: ${error}`;
    resultType.value = "error";
    addLog(`Error: ${error}`);
  } finally {
    isLoading.value = false;
  }
}

async function downloadBook() {
  if (!currentBookInfo.value || isDownloading.value) return;

  isDownloading.value = true;
  showPreview.value = false;
  resultMessage.value = "";
  resultType.value = "";
  downloadProgress.value = null;
  logs.value = [];

  try {
    addLog("Starting download...");
    const result = await invoke<string>("download_book", {
      url: bookUrl.value.trim(),
    });
    resultMessage.value = result;
    resultType.value = "success";
    addLog(result);
  } catch (error) {
    resultMessage.value = `${error}`;
    resultType.value = "error";
    addLog(`Error: ${error}`);
  } finally {
    isDownloading.value = false;
    downloadProgress.value = null;
    showAnimatedMessages.value = false;
    currentMessageIndex.value = 0;
  }
}

async function cancelDownload() {
  try {
    await invoke("cancel_download");
    showPreview.value = false;
    currentBookInfo.value = null;
    downloadProgress.value = null;
    showAnimatedMessages.value = false;
    addLog("Download cancelled");
  } catch (error) {
    addLog(`Cancel error: ${error}`);
  }
}

function resetAll() {
  showPreview.value = false;
  currentBookInfo.value = null;
  downloadProgress.value = null;
  resultMessage.value = "";
  resultType.value = "";
  showAnimatedMessages.value = false;
  currentMessageIndex.value = 0;
  logs.value = [];
}
</script>

<template>
  <div class="app-container">
    <!-- Header -->
    <header class="header">
      <div class="header-content">
        <div class="logo">
          <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M4 19.5A2.5 2.5 0 0 1 6.5 17H20" />
            <path d="M6.5 2H20v20H6.5A2.5 2.5 0 0 1 4 19.5v-15A2.5 2.5 0 0 1 6.5 2z" />
          </svg>
          <span>Rekhta Downloader</span>
        </div>
      </div>
    </header>

    <!-- Main Content -->
    <main class="main">
      <div class="content">
        <!-- Title -->
        <h1 class="title">Free Rekhta Books</h1>
        <p class="subtitle">Download any book from Rekhta as a high-quality PDF</p>

        <!-- URL Input -->
        <div class="input-wrapper">
          <input
            type="text"
            class="url-input"
            placeholder="Paste Rekhta ebook URL here..."
            v-model="bookUrl"
            @keydown.enter="previewBook"
            :disabled="isDownloading"
          />
          <button
            class="grab-btn"
            @click="previewBook"
            :disabled="isLoading || isDownloading"
            :title="isLoading ? 'Loading...' : 'Fetch book info'"
          >
            <svg v-if="isLoading" class="spinner" width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
              <path d="M21 12a9 9 0 1 1-6.219-8.56" />
            </svg>
            <svg v-else width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="7 10 12 15 17 10" />
              <line x1="12" y1="15" x2="12" y2="3" />
            </svg>
          </button>
        </div>

        <!-- Book Preview -->
        <Transition name="slide-fade">
          <div v-if="showPreview && currentBookInfo" class="preview-card">
            <div class="preview-inner">
              <img
                v-if="currentBookInfo.coverUrl"
                :src="currentBookInfo.coverUrl"
                alt="Book Cover"
                class="cover-image"
              />
              <div class="preview-info">
                <div>
                  <h2 class="book-title">
                    {{ currentBookInfo.title }}
                    <span v-if="currentBookInfo.author" class="book-author">
                      by {{ currentBookInfo.author }}
                    </span>
                  </h2>
                  <div class="book-meta">
                    <span v-if="currentBookInfo.year" class="meta-item">{{ currentBookInfo.year }}</span>
                    <span v-if="currentBookInfo.publisher" class="meta-item">{{ currentBookInfo.publisher }}</span>
                    <span class="meta-item pages-badge">{{ currentBookInfo.totalPages }} pages</span>
                  </div>
                </div>
                <div class="preview-actions">
                  <button class="btn btn-primary" @click="downloadBook" :disabled="isDownloading">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
                      <polyline points="7 10 12 15 17 10" />
                      <line x1="12" y1="15" x2="12" y2="3" />
                    </svg>
                    Download PDF
                  </button>
                  <button class="btn btn-secondary" @click="resetAll">
                    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                      <circle cx="12" cy="12" r="10" />
                      <line x1="15" y1="9" x2="9" y2="15" />
                      <line x1="9" y1="9" x2="15" y2="15" />
                    </svg>
                    Cancel
                  </button>
                </div>
              </div>
            </div>
          </div>
        </Transition>

        <!-- Progress Bar -->
        <Transition name="slide-fade">
          <div v-if="downloadProgress && isDownloading" class="progress-section">
            <div class="progress-bar-track">
              <div
                class="progress-bar-fill"
                :class="{ 'pulse-animation': isPdfPhase }"
                :style="{ width: progressPercentage + '%' }"
              >
                <span v-if="progressPercentage > 8" class="progress-text">{{ progressPercentage }}%</span>
              </div>
            </div>
            <div class="progress-message">
              <Transition name="fade" mode="out-in">
                <p v-if="showAnimatedMessages" :key="currentMessageIndex" class="status-msg">
                  {{ pdfCreationMessages[currentMessageIndex] }}
                </p>
                <p v-else class="status-msg" key="default">
                  {{ downloadProgress.message }}
                </p>
              </Transition>
            </div>
            <button class="btn btn-danger btn-sm" @click="cancelDownload">
              Cancel Download
            </button>
          </div>
        </Transition>

        <!-- Result Message -->
        <Transition name="slide-fade">
          <div v-if="resultMessage" class="result-banner" :class="resultType">
            <p>{{ resultMessage }}</p>
            <button class="btn-close" @click="resultMessage = ''">×</button>
          </div>
        </Transition>
      </div>
    </main>

    <!-- Footer -->
    <footer class="footer">
      <span>Rekhta Book Downloader</span>
      <span class="footer-dot">·</span>
      <span class="footer-muted">Desktop Edition</span>
    </footer>
  </div>
</template>

<style>
/* ═══════════════════════════════════════════
   Global Reset & Base Styles
   ═══════════════════════════════════════════ */
*,
*::before,
*::after {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

:root {
  --font-sans: "Inter", -apple-system, BlinkMacSystemFont, sans-serif;
  --font-mono: "JetBrains Mono", "SF Mono", "Fira Code", monospace;

  --green-50: #f0fdf4;
  --green-100: #dcfce7;
  --green-600: #16a34a;
  --green-700: #15803d;
  --green-800: #166534;
  --green-900: #14532d;

  --gray-50: #f9fafb;
  --gray-100: #f3f4f6;
  --gray-200: #e5e7eb;
  --gray-300: #d1d5db;
  --gray-400: #9ca3af;
  --gray-500: #6b7280;
  --gray-600: #4b5563;
  --gray-700: #374151;
  --gray-800: #1f2937;
  --gray-900: #111827;

  --red-500: #ef4444;
  --red-600: #dc2626;
  --red-50: #fef2f2;

  --radius-sm: 8px;
  --radius-md: 12px;
  --radius-lg: 16px;
  --radius-xl: 20px;

  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.05);
  --shadow-md: 0 4px 6px -1px rgba(0, 0, 0, 0.07), 0 2px 4px -2px rgba(0, 0, 0, 0.05);
  --shadow-lg: 0 10px 15px -3px rgba(0, 0, 0, 0.08), 0 4px 6px -4px rgba(0, 0, 0, 0.05);
}

html, body {
  height: 100%;
  width: 100%;
  overflow: hidden;
}

body {
  font-family: var(--font-sans);
  background: var(--gray-50);
  color: var(--gray-900);
  -webkit-font-smoothing: antialiased;
  -moz-osx-font-smoothing: grayscale;
}

#app {
  height: 100%;
}

/* ═══════════════════════════════════════════
   Layout
   ═══════════════════════════════════════════ */
.app-container {
  display: flex;
  flex-direction: column;
  min-height: 100vh;
  max-height: 100vh;
}

.header {
  padding: 16px 24px;
  border-bottom: 1px solid var(--gray-200);
  background: white;
  flex-shrink: 0;
  -webkit-app-region: drag;
}

.header-content {
  display: flex;
  align-items: center;
  justify-content: space-between;
  max-width: 720px;
  margin: 0 auto;
}

.logo {
  display: flex;
  align-items: center;
  gap: 10px;
  font-weight: 600;
  font-size: 15px;
  color: var(--green-800);
  letter-spacing: -0.01em;
}

.logo svg {
  opacity: 0.8;
}

.main {
  flex: 1;
  overflow-y: auto;
  padding: 40px 24px;
}

.content {
  max-width: 620px;
  margin: 0 auto;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 20px;
}

.footer {
  padding: 14px 24px;
  text-align: center;
  border-top: 1px solid var(--gray-200);
  background: white;
  flex-shrink: 0;
  font-size: 13px;
  color: var(--gray-500);
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}

.footer-dot {
  opacity: 0.4;
}

.footer-muted {
  opacity: 0.6;
}

/* ═══════════════════════════════════════════
   Typography
   ═══════════════════════════════════════════ */
.title {
  font-size: 32px;
  font-weight: 700;
  color: var(--green-900);
  letter-spacing: -0.03em;
  line-height: 1.1;
  text-align: center;
}

.subtitle {
  font-size: 15px;
  color: var(--gray-500);
  margin-top: -8px;
  text-align: center;
}

/* ═══════════════════════════════════════════
   URL Input
   ═══════════════════════════════════════════ */
.input-wrapper {
  display: flex;
  align-items: center;
  background: white;
  border: 1px solid var(--gray-300);
  border-radius: var(--radius-md);
  padding: 4px 4px 4px 16px;
  width: 100%;
  max-width: 500px;
  box-shadow: var(--shadow-sm);
  transition: border-color 0.2s, box-shadow 0.2s;
}

.input-wrapper:focus-within {
  border-color: var(--green-600);
  box-shadow: 0 0 0 3px rgba(22, 163, 74, 0.1);
}

.url-input {
  flex: 1;
  border: none;
  outline: none;
  font-size: 14px;
  font-family: var(--font-mono);
  color: var(--gray-800);
  background: transparent;
  min-width: 0;
}

.url-input::placeholder {
  color: var(--gray-400);
  font-family: var(--font-sans);
}

.url-input:disabled {
  opacity: 0.5;
}

.grab-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 40px;
  height: 40px;
  border: none;
  border-radius: var(--radius-sm);
  background: var(--green-700);
  color: white;
  cursor: pointer;
  transition: background 0.15s, transform 0.1s;
  flex-shrink: 0;
}

.grab-btn:hover:not(:disabled) {
  background: var(--green-800);
}

.grab-btn:active:not(:disabled) {
  transform: scale(0.95);
}

.grab-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.spinner {
  animation: spin 0.8s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* ═══════════════════════════════════════════
   Book Preview Card
   ═══════════════════════════════════════════ */
.preview-card {
  width: 100%;
  background: white;
  border: 1px solid var(--gray-200);
  border-radius: var(--radius-lg);
  padding: 20px;
  box-shadow: var(--shadow-md);
}

.preview-inner {
  display: flex;
  gap: 20px;
  align-items: flex-start;
}

.cover-image {
  width: 140px;
  height: auto;
  border-radius: var(--radius-sm);
  object-fit: cover;
  box-shadow: var(--shadow-lg);
  flex-shrink: 0;
}

.preview-info {
  flex: 1;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  min-height: 180px;
  gap: 16px;
}

.book-title {
  font-size: 22px;
  font-weight: 600;
  letter-spacing: -0.02em;
  line-height: 1.3;
  color: var(--gray-900);
}

.book-author {
  font-size: 16px;
  font-weight: 400;
  opacity: 0.45;
  display: inline;
  padding-left: 4px;
}

.book-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-top: 10px;
}

.meta-item {
  font-family: var(--font-mono);
  font-size: 13px;
  color: var(--gray-600);
  background: var(--gray-100);
  padding: 4px 10px;
  border-radius: 6px;
}

.pages-badge {
  background: var(--green-50);
  color: var(--green-800);
  font-weight: 500;
}

.preview-actions {
  display: flex;
  gap: 10px;
  flex-wrap: wrap;
}

/* ═══════════════════════════════════════════
   Buttons
   ═══════════════════════════════════════════ */
.btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 10px 16px;
  border: none;
  border-radius: var(--radius-sm);
  font-size: 14px;
  font-weight: 500;
  font-family: var(--font-sans);
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}

.btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-primary {
  background: var(--green-700);
  color: white;
}

.btn-primary:hover:not(:disabled) {
  background: var(--green-800);
}

.btn-secondary {
  background: var(--gray-100);
  color: var(--gray-700);
}

.btn-secondary:hover:not(:disabled) {
  background: var(--gray-200);
}

.btn-danger {
  background: transparent;
  color: var(--red-600);
  border: 1px solid var(--red-500);
}

.btn-danger:hover:not(:disabled) {
  background: var(--red-50);
}

.btn-sm {
  padding: 6px 14px;
  font-size: 13px;
}

.btn-close {
  background: none;
  border: none;
  font-size: 20px;
  cursor: pointer;
  color: inherit;
  opacity: 0.5;
  padding: 0 4px;
  line-height: 1;
}

.btn-close:hover {
  opacity: 1;
}

/* ═══════════════════════════════════════════
   Progress Bar
   ═══════════════════════════════════════════ */
.progress-section {
  width: 100%;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 14px;
}

.progress-bar-track {
  width: 100%;
  height: 28px;
  background: var(--gray-200);
  border-radius: 14px;
  overflow: hidden;
}

.progress-bar-fill {
  height: 100%;
  background: var(--green-700);
  border-radius: 14px;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: width 0.4s ease;
  min-width: 0;
}

.pulse-animation {
  animation: pulse-green 2s ease-in-out infinite;
}

@keyframes pulse-green {
  0%, 100% { background-color: var(--green-700); }
  50% { background-color: #22c55e; }
}

.progress-text {
  font-family: var(--font-mono);
  font-size: 12px;
  font-weight: 600;
  color: white;
}

.progress-message {
  text-align: center;
  min-height: 24px;
}

.status-msg {
  font-family: var(--font-mono);
  font-size: 14px;
  color: var(--gray-600);
  font-weight: 500;
}

/* ═══════════════════════════════════════════
   Result Banner
   ═══════════════════════════════════════════ */
.result-banner {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 14px 18px;
  border-radius: var(--radius-md);
  font-size: 14px;
}

.result-banner.success {
  background: var(--green-50);
  color: var(--green-800);
  border: 1px solid var(--green-100);
}

.result-banner.error {
  background: var(--red-50);
  color: var(--red-600);
  border: 1px solid #fecaca;
}

.result-banner p {
  flex: 1;
  word-break: break-word;
}

/* ═══════════════════════════════════════════
   Transitions
   ═══════════════════════════════════════════ */
.slide-fade-enter-active {
  transition: all 0.3s ease-out;
}

.slide-fade-leave-active {
  transition: all 0.2s ease-in;
}

.slide-fade-enter-from {
  opacity: 0;
  transform: translateY(12px);
}

.slide-fade-leave-to {
  opacity: 0;
  transform: translateY(-8px);
}

.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.3s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>