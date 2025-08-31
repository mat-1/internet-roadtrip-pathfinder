import { refreshPath } from ".";
import { clearCachedPaths, rerenderPath } from "./map/lines";

export const SETTINGS = {
    current_searching_path: true,
    allow_long_jumps: true,
    remove_reached_stops: false
};
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
                justify-content: space-between;
            }
        }
        `,
        className: "pathfinder-settings-tab-content",
    });

    function addToggle(
        label: string,
        key: keyof typeof SETTINGS,
        cb?: (value: boolean) => void
    ) {
        const id = `pathfinder-${key}`;

        const toggleEl = document.createElement("input");
        toggleEl.id = id;
        toggleEl.type = "checkbox";
        toggleEl.classList.add(IRF.ui.panel.styles.toggle);
        toggleEl.checked = SETTINGS[key];
        toggleEl.addEventListener("change", (e) => {
            const value = toggleEl.checked;
            SETTINGS[key] = value;
            saveSettings();
            cb?.(value);
        });

        const labelEl = document.createElement("label");
        labelEl.htmlFor = id;
        labelEl.textContent = label;

        const fieldGroupEl = document.createElement("div");
        fieldGroupEl.classList.add("field-group");

        fieldGroupEl.append(labelEl, toggleEl);

        settingsTab.container.appendChild(fieldGroupEl);
    }

    addToggle("Show currently searching path", "current_searching_path", () => {
        rerenderPath("current_searching_path");
    });
    addToggle("Allow long jumps", "allow_long_jumps", () => {
        clearCachedPaths();
        refreshPath();
    });
    addToggle("Remove stops as they are reached", "remove_reached_stops")
}
