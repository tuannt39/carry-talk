<script lang="ts">
  import "./app.css";
  import { onMount, onDestroy } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import logo from "$lib/assets/icons/logo.png";
  import TranscriptView from "$lib/components/TranscriptView.svelte";
  import Controls from "$lib/components/Controls.svelte";
  import SessionStatus from "$lib/components/SessionStatus.svelte";
  import SettingsPanel from "$lib/components/SettingsPanel.svelte";
  import { session } from "$lib/stores/session.svelte";
  import { transcript } from "$lib/stores/transcript.svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import { setLocale, t } from "$lib/i18n";
  import {
    getAudioRuntimeCapabilities,
    getSettings,
    getSessionState,
  } from "$lib/services/commands";
  import {
    onSessionError,
    onSessionRecovered,
    onSessionStateChanged,
    onTranscriptUpdate,
  } from "$lib/services/events";
  import type { UnlistenFn } from "@tauri-apps/api/event";
  import { devError, devWarn } from "$lib/utils/devLogger";

  type SettingsFocusTarget = "api-key" | null;
  type ToastKind = "info" | "success" | "warning" | "error";
  type ToastState = {
    id: number;
    kind: ToastKind;
    message: string;
  };

  let showSettings = $state(false);
  let settingsFocus = $state<SettingsFocusTarget>(null);
  let toast = $state<ToastState | null>(null);
  let unlisteners: UnlistenFn[] = [];
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  let nextToastId = 0;
  let isBootstrapping = $state(true);
  let settingsReady = $state(false);
  let sessionReady = $state(false);

  const startupStatusText = $derived.by(() => {
    if (!settingsReady) {
      return t("startup.loading_settings");
    }

    if (!sessionReady) {
      return t("startup.loading_session");
    }

    return t("startup.preparing_app");
  });

  onMount(() => {
    void initApp();
  });

  async function initApp() {
    const listenerResults = await Promise.allSettled([
      onSessionStateChanged((state) => {
        session.state = state;
      }),
      onTranscriptUpdate((payload) => {
        transcript.setAll(payload.segments);
      }),
      onSessionError((error) => {
        devError("Session error event:", error.message);
        const title = error.recoverable ? t("toast.session_warning") : t("toast.session_error");
        showToast(
          error.recoverable ? "warning" : "error",
          `${title}: ${error.message}`,
        );
      }),
      onSessionRecovered((summary) => {
        devWarn("Recovered interrupted session:", summary.session_id);
        showToast("success", t("toast.session_recovered"));
      }),
    ]);

    for (const result of listenerResults) {
      if (result.status === "fulfilled") {
        unlisteners.push(result.value);
      } else {
        devError("Failed to attach app listener:", result.reason);
      }
    }

    const settingsPromise = getSettings()
      .then((value) => {
        settings.current = value;
        settings.loaded = true;
        applyTheme(value.general.theme);
        setLocale(value.general.language);
      })
      .catch((error) => {
        devError("Failed to load settings:", error);
      })
      .finally(() => {
        settingsReady = true;
      });

    const sessionPromise = getSessionState()
      .then((value) => {
        session.state = value;
      })
      .catch((error) => {
        devError("Failed to load session state:", error);
      })
      .finally(() => {
        sessionReady = true;
      });

    const audioCapabilitiesPromise = getAudioRuntimeCapabilities()
      .then((value) => {
        settings.audioRuntimeCapabilities = value;
      })
      .catch((error) => {
        settings.audioRuntimeCapabilities = null;
        devError("Failed to load audio runtime capabilities:", error);
      })
      .finally(() => {
        settings.audioRuntimeCapabilitiesLoaded = true;
      });

    await Promise.allSettled([settingsPromise, sessionPromise, audioCapabilitiesPromise]);
    isBootstrapping = false;
  }

  onDestroy(() => {
    if (toastTimer) {
      clearTimeout(toastTimer);
    }

    for (const unlisten of unlisteners) {
      unlisten();
    }
  });

  function applyTheme(theme: string) {
    const savedTheme = theme === "light" ? "light" : "dark";

    if (theme === "light") {
      document.documentElement.setAttribute("data-theme", "light");
    } else {
      document.documentElement.removeAttribute("data-theme");
    }

    try {
      localStorage.setItem("carrytalk.theme", savedTheme);
    } catch (error) {
      devWarn("Failed to persist theme:", error);
    }
  }

  function dismissToast() {
    if (toastTimer) {
      clearTimeout(toastTimer);
      toastTimer = null;
    }

    toast = null;
  }

  function showToast(kind: ToastKind, message: string) {
    const id = ++nextToastId;
    const duration = kind === "info" || kind === "success" ? 3500 : 5000;

    if (toastTimer) {
      clearTimeout(toastTimer);
    }

    toast = { id, kind, message };
    toastTimer = setTimeout(() => {
      if (toast?.id === id) {
        toast = null;
      }
      toastTimer = null;
    }, duration);
  }

  function openSettings(focus: SettingsFocusTarget = null) {
    settingsFocus = focus;
    showSettings = true;
  }

  function handleSettingsClose() {
    showSettings = false;
    onSettingsChanged();
    settingsFocus = null;
  }

  function onSettingsChanged() {
    applyTheme(settings.current.general.theme);
    setLocale(settings.current.general.language);
  }

  async function handleHeaderMouseDown(event: MouseEvent) {
    if (event.button !== 0) {
      return;
    }

    const target = event.target as Element | null;
    if (target?.closest(".no-drag")) {
      return;
    }

    try {
      await getCurrentWindow().startDragging();
    } catch (error) {
      devError("Failed to start window dragging:", error);
    }
  }

  async function handleMinimize() {
    try {
      await getCurrentWindow().minimize();
    } catch (error) {
      devError("Failed to minimize window:", error);
    }
  }

  async function handleClose() {
    showSettings = false;
    settingsFocus = null;

    try {
      await getCurrentWindow().close();
    } catch (error) {
      devError("Failed to close window:", error);
    }
  }

  function handleContextMenu(event: MouseEvent) {
    const target =
      event.target instanceof Element
        ? event.target
        : event.target instanceof Node
          ? event.target.parentElement
          : null;

    if (
      target?.closest(
        'input, textarea, [contenteditable="true"], [data-selectable-text]',
      )
    ) {
      return;
    }

    event.preventDefault();
  }
</script>

<main class="flex flex-col h-screen overflow-hidden" oncontextmenu={handleContextMenu}>
  <div class="contents" inert={isBootstrapping} aria-hidden={isBootstrapping}>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <header
      class="flex items-center justify-between gap-3 px-3 py-2 border-b select-none transition-all duration-300"
      style="border-color: var(--border-color); background: var(--bg-secondary);"
      onmousedown={handleHeaderMouseDown}
      ondblclick={(event) => event.preventDefault()}
    >
      <div class="flex items-center gap-3 min-w-0">
        <img src={logo} alt="CarryTalk Logo" class="w-6 h-6 object-contain shrink-0" />
        <div class="flex items-center gap-2 min-w-0">
          <h1 class="text-base font-semibold tracking-tight shrink-0">{t("app_name")}</h1>
          <SessionStatus />
        </div>
      </div>

      <div class="no-drag flex items-center gap-1">
        <button
          id="btn-settings"
          type="button"
          onclick={() => openSettings()}
          class="window-control-button"
          title={t("controls.settings")}
          aria-label={t("controls.settings")}
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="18"
            height="18"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <path
              d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"
            />
            <circle cx="12" cy="12" r="3" />
          </svg>
        </button>

        <button
          id="btn-minimize"
          type="button"
          onclick={handleMinimize}
          class="window-control-button"
          title={t("controls.minimize")}
          aria-label={t("controls.minimize")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M5 12h14" />
          </svg>
        </button>

        <button
          id="btn-close"
          type="button"
          onclick={handleClose}
          class="window-control-button window-control-button-danger"
          title={t("controls.close_window")}
          aria-label={t("controls.close_window")}
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </svg>
        </button>
      </div>
    </header>

    <div class="relative flex-1 overflow-hidden">
      <TranscriptView />
    </div>

    <footer
      class="border-t px-4 py-3"
      style="border-color: var(--border-color); background: var(--bg-secondary);"
    >
      <Controls
        onNotify={showToast}
        onRequireApiKey={() => {
          openSettings("api-key");
        }}
      />
    </footer>
  </div>

  {#if isBootstrapping}
    <div class="startup-splash" role="status" aria-live="polite" aria-atomic="true">
      <div class="startup-splash__content">
        <h2 class="startup-splash__title">{t("startup.loading")}</h2>
        <p class="startup-splash__step">{startupStatusText}</p>
      </div>
    </div>
  {/if}

  {#if toast}
    <div class="toast-layer" aria-live="polite" aria-atomic="true">
      <div class={`app-toast app-toast-${toast.kind}`} role={toast.kind === "error" ? "alert" : "status"}>
        <div class="app-toast__content">
          <p class="app-toast__message">{toast.message}</p>
        </div>
        <button
          type="button"
          class="app-toast__close"
          aria-label={t("settings.close")}
          onclick={dismissToast}
        >
          <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <path d="M18 6 6 18" />
            <path d="m6 6 12 12" />
          </svg>
        </button>
      </div>
    </div>
  {/if}

  {#if showSettings}
    <SettingsPanel initialFocus={settingsFocus} onclose={handleSettingsClose} onNotify={showToast} />
  {/if}
</main>
