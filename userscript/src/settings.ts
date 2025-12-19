import { refreshPath } from ".";
import { pfWs } from "./api";
import { LOG_PREFIX } from "./constants";
import { clearCachedPaths, rerenderPath } from "./map/lines";
import { rerenderStopMarkers } from "./map/markers";
import { rerenderStopsMenu } from "./stops-menu";

export const DEFAULT_SETTINGS = {
    current_searching_path: true,
    allow_long_jumps: true,
    remove_reached_stops: false,
    show_stops_menu: false,
    // advanced settings
    use_option_cache: true,
    backend_url: "https://ir.matdoes.dev",
    heuristic_factor: 3.3,
    forward_penalty_on_intersections: 0,
    non_sharp_turn_penalty: 0,
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

// Load settings immediately on module initialization
loadSettingsFromSave();

export function initSettingsTab() {
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
            }
            .field-group-right-aligned {
                float: right;
                display: flex;
            }
            button {
                margin-left: 0.5rem;
            }
            i {
                margin-top: 0;
            }
            h2 {
                margin-bottom: 0;
            }
        }
        `,
        className: "pathfinder-settings-tab-content",
    });

    interface AddSettingOpts {
        inputEl: HTMLInputElement;
        /**
         * The label can either be a string, or a function that takes in the value and returns a string.
         */
        label:
            | string
            | ((
                  value: (typeof DEFAULT_SETTINGS)[keyof typeof DEFAULT_SETTINGS]
              ) => string);
        key: keyof typeof DEFAULT_SETTINGS;
        hasResetBtn: boolean;
        isInputSeparate?: boolean;
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
            else opts.inputEl.value = v.toString();
        }

        setDisplayedValue(SETTINGS[opts.key]);

        opts.inputEl.addEventListener("change", (e) => {
            let newValue: (typeof DEFAULT_SETTINGS)[typeof opts.key];
            const settingType = typeof DEFAULT_SETTINGS[opts.key];

            if (settingType === "boolean") newValue = opts.inputEl.checked;
            else if (settingType === "number")
                newValue = Number(opts.inputEl.value);
            else newValue = opts.inputEl.value;

            (SETTINGS as any)[opts.key] = newValue;
            updateLabel();

            saveSettings();
            opts.cb?.(newValue);
        });
        opts.inputEl.addEventListener("input", (e) => {
            updateLabel(opts.inputEl.value);
        });

        const labelEl = document.createElement("label");
        labelEl.htmlFor = id;

        function updateLabel(shownValue?: string) {
            const newLabelText =
                typeof opts.label === "string"
                    ? opts.label
                    : opts.label(shownValue ?? SETTINGS[opts.key]);
            labelEl.textContent = newLabelText;
        }

        updateLabel();
        const fieldGroupEl = document.createElement("div");
        fieldGroupEl.classList.add("field-group");
        fieldGroupEl.append(labelEl);

        const rightAlignedContentEl = document.createElement("div");
        rightAlignedContentEl.classList.add("field-group-right-aligned");
        if (!opts.isInputSeparate) rightAlignedContentEl.append(opts.inputEl);

        if (opts.hasResetBtn) {
            const resetBtnEl = document.createElement("button");
            resetBtnEl.textContent = "Reset";
            rightAlignedContentEl.append(resetBtnEl);
            resetBtnEl.addEventListener("click", () => {
                const newValue = DEFAULT_SETTINGS[opts.key];
                (SETTINGS as any)[opts.key] = newValue;
                setDisplayedValue(newValue);
                updateLabel();
                saveSettings();
                opts.cb?.(newValue);
            });
        }
        fieldGroupEl.append(rightAlignedContentEl);

        if (opts.isInputSeparate) fieldGroupEl.append(opts.inputEl);

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
        addSetting({ inputEl, label, key, hasResetBtn: true, cb });
    }
    function addTextInput(
        label: string,
        key: keyof typeof DEFAULT_SETTINGS,
        cb?: (value: (typeof DEFAULT_SETTINGS)[typeof key]) => void
    ) {
        const inputEl = document.createElement("input");
        addSetting({ inputEl, label, key, hasResetBtn: true, cb });
    }
    function addSlider(
        label: string,
        key: keyof typeof DEFAULT_SETTINGS,
        min: number,
        max: number,
        cb?: (value: (typeof DEFAULT_SETTINGS)[typeof key]) => void
    ) {
        const inputEl = document.createElement("input");
        inputEl.type = "range";
        inputEl.classList.add(IRF.ui.panel.styles.slider);
        inputEl.min = min.toString();
        inputEl.max = max.toString();
        if (max - min < 10) inputEl.step = "0.01";

        function getLabel(label: string, value: number) {
            return `${label}: ${value}`;
        }

        addSetting({
            inputEl,
            label: (value) => getLabel(label, value as number),
            key,
            hasResetBtn: true,
            isInputSeparate: true,
            cb: (value) => {
                cb?.(value);
            },
        });
    }

    addToggle("Show currently searching path", "current_searching_path", () => {
        rerenderPath("current_searching_path");
    });
    addToggle("Allow long jumps", "allow_long_jumps", () => {
        clearCachedPaths();
        refreshPath();
    });
    addToggle("Remove stops as they are reached", "remove_reached_stops");
    addToggle("Show stops menu", "show_stops_menu", () => {
        rerenderStopsMenu();
        rerenderStopMarkers();
    });

    const advancedSettingsHeaderEl = document.createElement("h2");
    advancedSettingsHeaderEl.textContent = "Advanced settings";
    const advancedSettingsDescEl = document.createElement("i");
    advancedSettingsDescEl.textContent =
        "NOTE: These settings can make the pathfinder stop working, mess up your ETAs, and significantly hurt performance. You should reset them if something breaks.";
    settingsTab.container.append(
        advancedSettingsHeaderEl,
        advancedSettingsDescEl
    );

    addTextInput("Custom backend URL", "backend_url", () => {
        pfWs.close();
        clearCachedPaths();
        refreshPath();
    });

    addToggle("Use option cache", "use_option_cache", () => {
        clearCachedPaths();
        refreshPath();
    });
    addSlider("Heuristic factor", "heuristic_factor", 1, 4, () => {
        clearCachedPaths();
        refreshPath();
    });
    addSlider(
        "Forward penalty on intersections (in seconds)",
        "forward_penalty_on_intersections",
        0,
        600,
        () => {
            clearCachedPaths();
            refreshPath();
        }
    );
    addSlider(
        "Non-sharp turn penalty (in seconds)",
        "non_sharp_turn_penalty",
        0,
        600,
        () => {
            clearCachedPaths();
            refreshPath();
        }
    );
}
