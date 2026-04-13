<script lang="ts">
  import { onMount } from "svelte";
  import { settings } from "$lib/stores/settings.svelte";
  import {
    hasApiKey,
    listAudioDevices,
    saveSettingsAndApiKey,
  } from "$lib/services/commands";
  import { t } from "$lib/i18n";
  import { devError } from "$lib/utils/devLogger";
  import type {
    AudioCapabilityReason,
    AudioCaptureMode,
    AudioDevice,
    AudioRuntimeCapabilities,
    AudioSourceCapability,
  } from "$lib/types";

  type Option = {
    value: string;
    label: string;
    disabled?: boolean;
  };

  const UI_LANGUAGE_OPTIONS: Option[] = [
    { value: "en", label: "English" },
    { value: "vi", label: "Tiếng Việt" },
  ];

  const PROVIDER_OPTIONS: Option[] = [{ value: "soniox", label: "Soniox" }];
  const AUDIO_CAPTURE_MODE_OPTIONS: Array<{
    value: AudioCaptureMode;
    labelKey: string;
  }> = [
    { value: "mic", labelKey: "settings.audio.capture_mode_options.microphone" },
    { value: "system", labelKey: "settings.audio.capture_mode_options.system_audio" },
    { value: "mixed", labelKey: "settings.audio.capture_mode_options.mixed" },
  ];

  const SONIOX_LANGUAGE_OPTIONS: Option[] = [
    { value: "af", label: "Afrikaans" },
    { value: "sq", label: "Albanian" },
    { value: "ar", label: "Arabic" },
    { value: "az", label: "Azerbaijani" },
    { value: "eu", label: "Basque" },
    { value: "be", label: "Belarusian" },
    { value: "bn", label: "Bengali" },
    { value: "bs", label: "Bosnian" },
    { value: "bg", label: "Bulgarian" },
    { value: "ca", label: "Catalan" },
    { value: "zh", label: "Chinese" },
    { value: "hr", label: "Croatian" },
    { value: "cs", label: "Czech" },
    { value: "da", label: "Danish" },
    { value: "nl", label: "Dutch" },
    { value: "en", label: "English" },
    { value: "et", label: "Estonian" },
    { value: "fi", label: "Finnish" },
    { value: "fr", label: "French" },
    { value: "gl", label: "Galician" },
    { value: "de", label: "German" },
    { value: "el", label: "Greek" },
    { value: "gu", label: "Gujarati" },
    { value: "he", label: "Hebrew" },
    { value: "hi", label: "Hindi" },
    { value: "hu", label: "Hungarian" },
    { value: "id", label: "Indonesian" },
    { value: "it", label: "Italian" },
    { value: "ja", label: "Japanese" },
    { value: "kn", label: "Kannada" },
    { value: "kk", label: "Kazakh" },
    { value: "ko", label: "Korean" },
    { value: "lv", label: "Latvian" },
    { value: "lt", label: "Lithuanian" },
    { value: "mk", label: "Macedonian" },
    { value: "ms", label: "Malay" },
    { value: "ml", label: "Malayalam" },
    { value: "mr", label: "Marathi" },
    { value: "no", label: "Norwegian" },
    { value: "fa", label: "Persian" },
    { value: "pl", label: "Polish" },
    { value: "pt", label: "Portuguese" },
    { value: "pa", label: "Punjabi" },
    { value: "ro", label: "Romanian" },
    { value: "ru", label: "Russian" },
    { value: "sr", label: "Serbian" },
    { value: "sk", label: "Slovak" },
    { value: "sl", label: "Slovenian" },
    { value: "es", label: "Spanish" },
    { value: "sw", label: "Swahili" },
    { value: "sv", label: "Swedish" },
    { value: "tl", label: "Tagalog" },
    { value: "ta", label: "Tamil" },
    { value: "te", label: "Telugu" },
    { value: "th", label: "Thai" },
    { value: "tr", label: "Turkish" },
    { value: "uk", label: "Ukrainian" },
    { value: "ur", label: "Urdu" },
    { value: "vi", label: "Vietnamese" },
    { value: "cy", label: "Welsh" },
  ];

  type NotifyKind = "info" | "success" | "warning" | "error";

  let {
    onclose,
    onNotify,
    initialFocus = null,
  }: {
    onclose: () => void;
    onNotify: (kind: NotifyKind, message: string) => void;
    initialFocus?: "api-key" | null;
  } = $props();

  const MASKED_API_KEY = "••••••••••••";

  let apiKeyDraft = $state("");
  let apiKeyExists = $state(false);
  let apiKeyTouched = $state(false);
  let apiKeyStatusLoaded = $state(false);
  let saving = $state(false);
  let showAdvancedAudio = $state(false);
  let availableAudioDevices = $state<AudioDevice[]>([]);
  let audioDevicesLoaded = $state(false);
  let localSettings = $state($state.snapshot(settings.current));
  let runtimeCapabilities = $state<AudioRuntimeCapabilities | null>(settings.audioRuntimeCapabilities);

  let apiKeySection = $state<HTMLElement | undefined>(undefined);
  let apiKeyInputElement = $state<HTMLInputElement | undefined>(undefined);

  const themeOptions = $derived<Option[]>([
    { value: "dark", label: t("settings.dark") },
    { value: "light", label: t("settings.light") },
  ]);

  const sourceLanguageOptions = $derived<Option[]>([
    { value: "auto", label: t("settings.auto_detect") },
    ...SONIOX_LANGUAGE_OPTIONS,
  ]);

  const microphoneCapability = $derived(runtimeCapabilities?.microphone ?? null);
  const systemOutputCapability = $derived(runtimeCapabilities?.system_output ?? null);

  const micDeviceOptions = $derived.by<Option[]>(() => {
    if (microphoneCapability) {
      return [
        { value: "default", label: t("settings.audio.default_device") },
        ...microphoneCapability.devices.map((device) => ({
          value: device.id,
          label: formatDeviceLabel(device.label, device.is_default),
          disabled: !device.usable,
        })),
      ];
    }

    return [
      { value: "default", label: t("settings.audio.default_device") },
      ...availableAudioDevices.map((device) => ({
        value: device.name,
        label: formatDeviceLabel(device.name, device.is_default),
      })),
    ];
  });

  const systemDeviceOptions = $derived.by<Option[]>(() => {
    if (systemOutputCapability) {
      return [
        { value: "default", label: t("settings.audio.default_device") },
        ...systemOutputCapability.devices.map((device) => ({
          value: device.id,
          label: formatDeviceLabel(device.label, device.is_default),
        })),
      ];
    }

    return [{ value: "default", label: t("settings.audio.default_device") }];
  });

  const availableCaptureModeOptions = $derived.by(() => {
    return AUDIO_CAPTURE_MODE_OPTIONS.map((option) => ({
      value: option.value,
      label: t(option.labelKey),
    }));
  });

  const needsMicDevice = $derived(
    localSettings.audio.capture_mode === "mic" || localSettings.audio.capture_mode === "mixed",
  );

  const needsSystemDevice = $derived(
    localSettings.audio.capture_mode === "system" ||
      localSettings.audio.capture_mode === "mixed",
  );

  const microphoneCapabilityReason = $derived(
    localizeAudioReason(microphoneCapability?.reason ?? null),
  );

  const systemOutputCapabilityReason = $derived(
    localizeAudioReason(systemOutputCapability?.reason ?? null),
  );

  const mixedCapabilityReason = $derived(
    localizeAudioReason(runtimeCapabilities?.mixed_reason ?? null),
  );

  const selectedModeCapabilityNote = $derived.by(() => {
    if (!runtimeCapabilities) {
      return null;
    }

    if (localSettings.audio.capture_mode === "mic") {
      return microphoneCapabilityReason;
    }

    if (localSettings.audio.capture_mode === "system") {
      return systemOutputCapabilityReason;
    }

    return mixedCapabilityReason;
  });

  function formatDeviceLabel(label: string, isDefault: boolean): string {
    return isDefault ? `${label} ${t("settings.audio.default_suffix")}` : label;
  }

  function localizeAudioReason(reason: AudioCapabilityReason | null): {
    message: string;
    detail: string | null;
  } | null {
    if (!reason) {
      return null;
    }

    const translated = t(`settings.audio.reasons.${reason.code}`);
    return {
      message:
        translated === `settings.audio.reasons.${reason.code}`
          ? t("settings.audio.reasons.audio.generic_unavailable")
          : translated,
      detail: reason.detail ?? null,
    };
  }

  function applyApiKeyStatus(existingApiKey: boolean): void {
    apiKeyExists = existingApiKey;
    apiKeyTouched = false;
    apiKeyDraft = existingApiKey ? MASKED_API_KEY : "";
  }

  function isSourceUsable(capability: AudioSourceCapability | null): boolean {
    if (!capability) {
      return true;
    }

    return capability.supported && capability.usable;
  }

  function getFirstUsableDeviceId(capability: AudioSourceCapability | null): string {
    const usableDefault = capability?.devices.find((device) => device.usable && device.is_default);
    if (usableDefault) {
      return usableDefault.id;
    }

    const defaultDevice = capability?.devices.find((device) => device.is_default);
    if (defaultDevice) {
      return defaultDevice.id;
    }

    const usableDevice = capability?.devices.find((device) => device.usable);
    if (usableDevice) {
      return usableDevice.id;
    }

    const firstDevice = capability?.devices[0];
    if (firstDevice) {
      return firstDevice.id;
    }

    return "default";
  }

  function ensureSupportedCaptureMode(): void {
    if (
      localSettings.audio.capture_mode === "mic" ||
      localSettings.audio.capture_mode === "system" ||
      localSettings.audio.capture_mode === "mixed"
    ) {
      return;
    }

    localSettings.audio.capture_mode = "mic";
  }

  function normalizeCapabilityDeviceSelections(): void {
    if (microphoneCapability) {
      const micDeviceExists =
        localSettings.audio.mic_device_id === "default" ||
        microphoneCapability.devices.some(
          (device) => device.id === localSettings.audio.mic_device_id && device.usable,
        );

      if (!micDeviceExists) {
        localSettings.audio.mic_device_id = getFirstUsableDeviceId(microphoneCapability);
      }
    }

    if (systemOutputCapability) {
      const systemDeviceExists =
        localSettings.audio.system_device_id === "default" ||
        systemOutputCapability.devices.some(
          (device) => device.id === localSettings.audio.system_device_id,
        );

      if (!systemDeviceExists) {
        localSettings.audio.system_device_id = getFirstUsableDeviceId(systemOutputCapability);
      }
    }
  }

  function normalizeAudioSettings(): void {
    localSettings.audio.mic_device_id =
      localSettings.audio.mic_device_id.trim() || "default";
    localSettings.audio.system_device_id =
      localSettings.audio.system_device_id.trim() || "default";
    localSettings.audio.mic_gain = Math.min(
      8,
      Math.max(0.1, Number(localSettings.audio.mic_gain) || 1),
    );
    localSettings.audio.system_gain = Math.min(
      8,
      Math.max(0.1, Number(localSettings.audio.system_gain) || 1),
    );
    localSettings.audio.mic_silence_threshold = Math.min(
      32767,
      Math.max(1, Math.round(Number(localSettings.audio.mic_silence_threshold) || 800)),
    );
    localSettings.audio.system_silence_threshold = Math.min(
      32767,
      Math.max(1, Math.round(Number(localSettings.audio.system_silence_threshold) || 800)),
    );
  }

  onMount(async () => {
    runtimeCapabilities = settings.audioRuntimeCapabilities;

    try {
      const [existingApiKey, devices] = await Promise.all([
        hasApiKey(),
        listAudioDevices(),
      ]);
      applyApiKeyStatus(existingApiKey);
      availableAudioDevices = devices;
      ensureSupportedCaptureMode();
      normalizeCapabilityDeviceSelections();
    } catch (error) {
      applyApiKeyStatus(false);
      devError("Failed to load settings panel state:", error);
      onNotify("error", formatError(error));
    } finally {
      apiKeyStatusLoaded = true;
      audioDevicesLoaded = true;
    }

    if (initialFocus === "api-key") {
      requestAnimationFrame(() => {
        apiKeySection?.scrollIntoView({ block: "center", behavior: "smooth" });
        apiKeyInputElement?.focus();
      });
    }
  });

  $effect(() => {
    runtimeCapabilities = settings.audioRuntimeCapabilities;
    ensureSupportedCaptureMode();
    normalizeCapabilityDeviceSelections();
  });

  function formatError(error: unknown): string {
    const message = error instanceof Error ? error.message : String(error);
    const normalized = message.replace(/^Error:\s*/, "");
    const authMatch = normalized.match(/^Auth\((.*)\)$/);
    return authMatch ? authMatch[1] : normalized;
  }

  async function handleSave() {
    saving = true;

    try {
      normalizeAudioSettings();
      const nextSettings = $state.snapshot(localSettings);
      const trimmedApiKey = apiKeyDraft.trim();
      const nextApiKey =
        apiKeyTouched && trimmedApiKey.length > 0 && trimmedApiKey !== MASKED_API_KEY
          ? trimmedApiKey
          : undefined;
      await saveSettingsAndApiKey(nextSettings, nextApiKey);
      settings.current = nextSettings;
      applyApiKeyStatus(await hasApiKey());

      onNotify("success", t("settings.saved_success"));

      onclose();
    } catch (error) {
      devError("Failed to save settings:", error);
      onNotify("error", formatError(error));
    } finally {
      saving = false;
    }
  }
</script>

<!-- Backdrop -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed inset-0 z-50 flex items-center justify-center"
  style="background: rgba(0,0,0,0.5); backdrop-filter: blur(4px);"
  onkeydown={(event) => event.key === "Escape" && onclose()}
>
  <!-- Panel -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="w-full max-w-lg mx-4 rounded-2xl shadow-2xl overflow-hidden border"
    style="background: var(--bg-secondary); border-color: var(--border-color);"
    onclick={(event) => event.stopPropagation()}
    onkeydown={(event) => event.stopPropagation()}
  >
    <div
      class="flex items-center justify-between px-5 py-4 border-b"
      style="border-color: var(--border-color);"
    >
      <h2 class="text-sm font-semibold">{t("settings.title")}</h2>
      <button
        type="button"
        onclick={onclose}
        class="window-control-button"
        aria-label={t("settings.close")}
      >
        <svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M18 6 6 18" />
          <path d="m6 6 12 12" />
        </svg>
      </button>
    </div>

    <div class="px-5 py-5 space-y-6 max-h-[70vh] overflow-y-auto">
      <section class="space-y-4">
        <h3 class="settings-section-title">{t("settings.provider")}</h3>

        <label class="block space-y-1.5">
          <span class="text-sm font-medium">{t("settings.provider_name")}</span>
          <select
            bind:value={localSettings.provider.name}
            class="settings-control"
            aria-label={t("settings.provider_name")}
          >
            {#each PROVIDER_OPTIONS as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>

        <div bind:this={apiKeySection} class="space-y-2" id="settings-api-key-section">
          <label class="block space-y-1.5">
            <span class="text-sm font-medium">{t("settings.api_key")}</span>
            <input
              bind:this={apiKeyInputElement}
              type="password"
              value={apiKeyDraft}
              onfocus={() => {
                if (apiKeyExists && !apiKeyTouched && apiKeyDraft === MASKED_API_KEY) {
                  apiKeyDraft = "";
                }
              }}
              onblur={() => {
                if (apiKeyExists && apiKeyDraft.trim().length === 0) {
                  apiKeyDraft = MASKED_API_KEY;
                  apiKeyTouched = false;
                }
              }}
              oninput={(event) => {
                apiKeyTouched = true;
                apiKeyDraft = event.currentTarget.value;
              }}
              placeholder={apiKeyStatusLoaded ? t("settings.api_key_placeholder") : "…"}
              class="settings-control"
              autocomplete="new-password"
              disabled={!apiKeyStatusLoaded}
              aria-describedby="settings-api-key-hint"
            />
          </label>
        </div>

        <label class="block space-y-1.5">
          <span class="text-sm font-medium">{t("settings.source_language")}</span>
          <select
            bind:value={localSettings.provider.source_language}
            class="settings-control"
            aria-label={t("settings.source_language")}
          >
            {#each sourceLanguageOptions as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>

        <label class="block space-y-1.5">
          <span class="text-sm font-medium">{t("settings.target_language")}</span>
          <select
            bind:value={localSettings.provider.translation_target_language}
            class="settings-control"
            aria-label={t("settings.target_language")}
          >
            {#each SONIOX_LANGUAGE_OPTIONS as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>
      </section>

      <section class="space-y-4">
        <h3 class="settings-section-title">{t("settings.general")}</h3>

        <label class="block space-y-1.5">
          <span class="text-sm font-medium">{t("settings.theme")}</span>
          <select
            bind:value={localSettings.general.theme}
            class="settings-control"
            aria-label={t("settings.theme")}
          >
            {#each themeOptions as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>

        <label class="block space-y-1.5">
          <span class="text-sm font-medium">{t("settings.ui_language")}</span>
          <select
            bind:value={localSettings.general.language}
            class="settings-control"
            aria-label={t("settings.ui_language")}
          >
            {#each UI_LANGUAGE_OPTIONS as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>
      </section>

      <section class="space-y-4">
        <h3 class="settings-section-title">{t("settings.audio.title")}</h3>

        <label class="block space-y-1.5">
          <span class="text-sm font-medium">{t("settings.audio.capture_mode")}</span>
          <select
            bind:value={localSettings.audio.capture_mode}
            class="settings-control"
            aria-label={t("settings.audio.capture_mode")}
          >
            {#each availableCaptureModeOptions as option}
              <option value={option.value}>{option.label}</option>
            {/each}
          </select>
        </label>

        {#if runtimeCapabilities}
          <div class="space-y-2 rounded-xl border p-3 text-xs" style="border-color: var(--border-color);">
            <div class="flex items-start justify-between gap-3">
              <span class="font-medium">{t("settings.audio.microphone")}</span>
              <span>{microphoneCapability?.usable ? t("settings.audio.available") : t("settings.audio.unavailable")}</span>
            </div>
            {#if microphoneCapabilityReason}
              <p style="color: var(--text-secondary);">{microphoneCapabilityReason.message}</p>
              {#if microphoneCapabilityReason.detail}
                <p style="color: var(--text-secondary);">{microphoneCapabilityReason.detail}</p>
              {/if}
            {/if}

            <div class="flex items-start justify-between gap-3 pt-2 border-t" style="border-color: var(--border-color);">
              <span class="font-medium">{t("settings.audio.system_audio")}</span>
              <span>{systemOutputCapability?.usable ? t("settings.audio.available") : t("settings.audio.unavailable")}</span>
            </div>
            {#if systemOutputCapabilityReason}
              <p style="color: var(--text-secondary);">{systemOutputCapabilityReason.message}</p>
              {#if systemOutputCapabilityReason.detail}
                <p style="color: var(--text-secondary);">{systemOutputCapabilityReason.detail}</p>
              {/if}
            {/if}

            <div class="flex items-start justify-between gap-3 pt-2 border-t" style="border-color: var(--border-color);">
              <span class="font-medium">{t("settings.audio.mixed_mode")}</span>
              <span>{runtimeCapabilities.mixed_supported ? t("settings.audio.available") : t("settings.audio.unavailable")}</span>
            </div>
            {#if mixedCapabilityReason}
              <p style="color: var(--text-secondary);">{mixedCapabilityReason.message}</p>
              {#if mixedCapabilityReason.detail}
                <p style="color: var(--text-secondary);">{mixedCapabilityReason.detail}</p>
              {/if}
            {/if}
          </div>
        {/if}

        {#if selectedModeCapabilityNote}
          <div class="text-xs space-y-1" style="color: var(--text-secondary);">
            <p>{selectedModeCapabilityNote.message}</p>
            {#if selectedModeCapabilityNote.detail}
              <p>{selectedModeCapabilityNote.detail}</p>
            {/if}
          </div>
        {/if}

        {#if needsMicDevice}
          <label class="block space-y-1.5">
            <span class="text-sm font-medium">{t("settings.audio.microphone_device")}</span>
            <select
              bind:value={localSettings.audio.mic_device_id}
              class="settings-control"
              aria-label={t("settings.audio.microphone_device")}
              disabled={!audioDevicesLoaded || (runtimeCapabilities !== null && !isSourceUsable(microphoneCapability))}
            >
              {#each micDeviceOptions as option}
                <option value={option.value} disabled={option.disabled}>{option.label}</option>
              {/each}
            </select>
          </label>
        {/if}

        {#if needsSystemDevice}
          <label class="block space-y-1.5">
            <span class="text-sm font-medium">{t("settings.audio.system_audio_device")}</span>
            <select
              bind:value={localSettings.audio.system_device_id}
              class="settings-control"
              aria-label={t("settings.audio.system_audio_device")}
              disabled={!audioDevicesLoaded || (runtimeCapabilities !== null && systemOutputCapability?.devices.length === 0)}
            >
              {#each systemDeviceOptions as option}
                <option value={option.value} disabled={option.disabled}>{option.label}</option>
              {/each}
            </select>
          </label>
        {/if}

        <div class="space-y-3 rounded-xl border p-3" style="border-color: var(--border-color);">
          <button
            type="button"
            class="text-sm font-medium underline-offset-2 hover:underline"
            onclick={() => {
              showAdvancedAudio = !showAdvancedAudio;
            }}
            aria-expanded={showAdvancedAudio}
          >
            {showAdvancedAudio
              ? t("settings.audio.hide_advanced")
              : t("settings.audio.show_advanced")}
          </button>

          {#if showAdvancedAudio}
            <div class="grid gap-4 md:grid-cols-2">
              <label class="block space-y-1.5">
                <span class="text-sm font-medium">{t("settings.audio.mic_gain")}</span>
                <input
                  bind:value={localSettings.audio.mic_gain}
                  type="number"
                  min="0.1"
                  max="8"
                  step="0.1"
                  class="settings-control"
                  aria-label={t("settings.audio.mic_gain")}
                />
              </label>

              <label class="block space-y-1.5">
                <span class="text-sm font-medium">{t("settings.audio.system_gain")}</span>
                <input
                  bind:value={localSettings.audio.system_gain}
                  type="number"
                  min="0.1"
                  max="8"
                  step="0.1"
                  class="settings-control"
                  aria-label={t("settings.audio.system_gain")}
                />
              </label>

              <label class="block space-y-1.5">
                <span class="text-sm font-medium">{t("settings.audio.mic_silence_threshold")}</span>
                <input
                  bind:value={localSettings.audio.mic_silence_threshold}
                  type="number"
                  min="1"
                  max="32767"
                  step="1"
                  class="settings-control"
                  aria-label={t("settings.audio.mic_silence_threshold")}
                />
              </label>

              <label class="block space-y-1.5">
                <span class="text-sm font-medium">{t("settings.audio.system_silence_threshold")}</span>
                <input
                  bind:value={localSettings.audio.system_silence_threshold}
                  type="number"
                  min="1"
                  max="32767"
                  step="1"
                  class="settings-control"
                  aria-label={t("settings.audio.system_silence_threshold")}
                />
              </label>
            </div>
          {/if}
        </div>
      </section>
    </div>

    <div
      class="flex justify-end items-center gap-2 px-5 py-4 border-t"
      style="border-color: var(--border-color);"
    >
      <button
        type="button"
        onclick={onclose}
        class="px-4 py-2 rounded-xl text-sm transition-colors hover:bg-white/5"
      >
        {t("settings.close")}
      </button>
      <button
        type="button"
        onclick={handleSave}
        disabled={saving}
        class="px-4 py-2 rounded-xl text-sm font-medium text-white transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
        style="background: var(--color-accent-500);"
      >
        {saving ? "…" : t("settings.save")}
      </button>
    </div>
  </div>
</div>
