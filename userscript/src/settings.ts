import { refreshPath } from ".";
import { pfWs } from "./api";
import { LOG_PREFIX } from "./constants";
import { clearCachedPaths, rerenderPath } from "./map/lines";

export const DEFAULT_SETTINGS = {
    current_searching_path: true,
    allow_long_jumps: true,
    remove_reached_stops: false,
    backend_url: "https://ir.matdoes.dev",
};

export const SETTINGS = JSON.parse(JSON.stringify(DEFAULT_SETTINGS));
function loadSettingsFromSave() {
    const savedSettings: string = GM_getValue("settings") ?? "{}";
    for (const [k, v] of Object.entries(JSON.parse(savedSettings))) {
        // @ts-ignore assume that settings has the correct types
        SETTINGS[k] = v;
    }
}
function saveSettings() {
    GM_setValue("settings", JSON.stringify(SETTINGS));
}

export function initSettingsTab() {
    loadSettingsFromSave();
    console.log(LOG_PREFIX, "loaded settings:", SETTINGS);

    const settingsTab = IRF.ui.panel.createTabFor(GM.info, {
        tabName: "Pathfinder",
        style: `
        .pathfinder-settings-tab-content {
            *, *::before, *::after {
                box-sizing: border-box;
            }

            .field-group {
                margin-block: 1rem;
                gap: 0.25rem;
                display: flex;
                align-items: center;
            }
            .field-group label {
                flex: 1;
            }
        }
        `,
        className: "pathfinder-settings-tab-content",
    });

    interface AddSettingOpts {
        inputEl: HTMLInputElement;
        label: string;
        key: keyof typeof DEFAULT_SETTINGS;
        hasResetBtn: boolean;
        cb?: (
            value: (typeof DEFAULT_SETTINGS)[keyof typeof DEFAULT_SETTINGS]
        ) => void;
    }

    function addSetting(opts: AddSettingOpts) {
        const id = `pathfinder-${opts.key}`;

        opts.inputEl.id = id;

        function setDisplayedValue(
            v: (typeof DEFAULT_SETTINGS)[typeof opts.key]
        ) {
            if (typeof v === "boolean") opts.inputEl.checked = v;
            else opts.inputEl.value = v;
        }

        setDisplayedValue(SETTINGS[opts.key]);

        opts.inputEl.addEventListener("change", (e) => {
            let newValue: (typeof DEFAULT_SETTINGS)[typeof opts.key];
            if (typeof DEFAULT_SETTINGS[opts.key] === "boolean")
                newValue = opts.inputEl.checked;
            else newValue = opts.inputEl.value;
            (SETTINGS as any)[opts.key] = newValue;

            saveSettings();
            opts.cb?.(newValue);
        });

        const labelEl = document.createElement("label");
        labelEl.htmlFor = id;
        labelEl.textContent = opts.label;
        const fieldGroupEl = document.createElement("div");
        fieldGroupEl.classList.add("field-group");
        fieldGroupEl.append(labelEl, opts.inputEl);

        if (opts.hasResetBtn) {
            const resetBtnEl = document.createElement("button");
            resetBtnEl.textContent = "Reset";
            fieldGroupEl.append(resetBtnEl);
            resetBtnEl.addEventListener("click", () => {
                const newValue = DEFAULT_SETTINGS[opts.key];
                (SETTINGS as any)[opts.key] = newValue;
                setDisplayedValue(newValue);
                saveSettings();
                opts.cb?.(newValue);
            });
        }

        settingsTab.container.appendChild(fieldGroupEl);
    }

    function addToggle(
        label: string,
        key: keyof typeof DEFAULT_SETTINGS,
        cb?: (value: (typeof DEFAULT_SETTINGS)[typeof key]) => void
    ) {
        const inputEl = document.createElement("input");
        inputEl.type = "checkbox";
        inputEl.classList.add(IRF.ui.panel.styles.toggle);
        addSetting({ inputEl, label, key, hasResetBtn: false, cb });
    }
    function addTextInput(
        label: string,
        key: keyof typeof DEFAULT_SETTINGS,
        cb?: (value: (typeof DEFAULT_SETTINGS)[typeof key]) => void
    ) {
        const inputEl = document.createElement("input");
        addSetting({ inputEl, label, key, hasResetBtn: true, cb });
    }

    addToggle("Show currently searching path", "current_searching_path", () => {
        rerenderPath("current_searching_path");
    });
    addToggle("Allow long jumps", "allow_long_jumps", () => {
        clearCachedPaths();
        refreshPath();
    });
    addToggle("Remove stops as they are reached", "remove_reached_stops");

    addTextInput("Custom backend URL", "backend_url", () => {
        pfWs.close();
        clearCachedPaths();
        refreshPath();
    });
}
