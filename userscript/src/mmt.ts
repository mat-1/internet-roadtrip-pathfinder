import { replacePathfinderInfoEl, updateDestinationFromString } from ".";
import { map } from "./map";
import { getLat, getLng, newPosition } from "./pos";
import { addStopToPath, removeStop } from "./stops";

export let tricksControl: MinimapTricksControl | undefined = undefined;

/**
 * Tries to initialize the Minimap Tricks integration, if possible.
 */
export function tryInitMmt() {
    Promise.all([waitForMmtControl, waitForMmtAddContextFn]).then(
        ([newTricksControl, addContext]) => {
            tricksControl = newTricksControl;
            onMmtFound(addContext);
        }
    );
}

/**
 * Called when the Minimap Tricks userscript is found.
 */
async function onMmtFound(addContext: MinimapTricksAddContextFn) {
    if (!tricksControl) {
        throw Error("tricksControl must be set");
    }

    document.body.classList.add("pathfinder-found-minimap-tricks");

    function setAndSaveDestination(pos: GeoJSON.Position) {
        updateDestinationFromString(`${getLat(pos)},${getLng(pos)}`);
    }

    function clearAndSaveDestination() {
        updateDestinationFromString("");
    }

    // Map button
    tricksControl.addButton(
        GM_getResourceURL("flagSvg"),
        "Find path to location",
        (control) =>
            setAndSaveDestination(newPosition(control.lat, control.lng)),
        // contexts
        ["Map"]
    );

    // Add stop button
    const addStopBtn = tricksControl.addButton(
        GM_getResourceURL("flagSvg"),
        "Add stop to path",
        (control) => {
            addStopToPath(newPosition(control.lat, control.lng));
        },
        // contexts
        ["Map", "Marker", "Pathfinder"]
    );
    addStopBtn.context_button.classList.add(
        "pathfinder-add-stop-mmt-context-menu-button"
    );

    // Marker button
    tricksControl.addButton(
        GM_getResourceURL("flagSvg"),
        "Set as pathfinder destination",
        (control) =>
            setAndSaveDestination(newPosition(control.lat, control.lng)),
        // contexts
        ["Marker"]
    );

    // Remove buttons
    const removePathBtn = tricksControl.addButton(
        GM_getResourceURL("flagWithCrossSvg"),
        "Clear found path",
        () => clearAndSaveDestination(),
        // contexts
        ["Side", "Map", "Car", "Pathfinder", "Pathfinder destination"]
    );
    removePathBtn.side_button.classList.add(
        "pathfinder-clear-path-mmt-side-button"
    );
    removePathBtn.context_button.classList.add(
        "pathfinder-clear-path-mmt-context-menu-button"
    );

    tricksControl.addButton(
        GM_getResourceURL("flagWithCrossSvg"),
        "Remove stop",
        (control) => {
            removeStop(newPosition(control.lat, control.lng));
        },
        // contexts
        ["Pathfinder stop"]
    );

    addContext("Pathfinder", [
        // New buttons
        "Find path to location",
        "Add stop to path",
        "Clear found path",
        // Grandfathered buttons from Minimap Tricks
        "Copy coordinates",
        "Add marker",
    ]);
    addContext("Pathfinder destination", [
        "Clear found path",

        "Copy coordinates",
        "Add marker",
    ]);
    addContext("Pathfinder stop", [
        "Remove stop",

        "Copy coordinates",
        "Add marker",
    ]);

    map.on("contextmenu", "best_path", (event) => {
        event.preventDefault();
        openContextMenu(event.originalEvent, event.lngLat, "Pathfinder");
    });
    map.on("contextmenu", "best_path_segments", (event) => {
        event.preventDefault();
        openContextMenu(event.originalEvent, event.lngLat, "Pathfinder");
    });

    setNewInfoDisplay();
}

function setNewInfoDisplay() {
    const pathfinderInfoControl = new (class implements maplibregl.IControl {
        _map?: maplibregl.Map;
        _container?: HTMLDivElement;

        onAdd(map: maplibregl.Map) {
            this._map = map;
            this._container = this.insertDom();
            return this._container;
        }

        insertDom() {
            const containerEl = document.createElement("div");
            containerEl.id = "minimap-controls";
            containerEl.className =
                "maplibregl-ctrl maplibregl-ctrl-scale pathfinder-info";
            containerEl.style.marginRight = "36px";

            return containerEl;
        }

        onRemove() {
            if (this._container) {
                this._container.parentNode?.removeChild(this._container);
            }
            this._map = undefined;
        }
    })();

    map.addControl(pathfinderInfoControl, "bottom-right");

    replacePathfinderInfoEl(pathfinderInfoControl._container!);
}

const waitForMmtControl = new Promise<MinimapTricksControl>((resolve) => {
    if (unsafeWindow._MMT_control) {
        resolve(unsafeWindow._MMT_control);
        return;
    }

    let _tricksControl: MinimapTricksControl;
    Object.defineProperty(unsafeWindow, "_MMT_control", {
        get() {
            return _tricksControl;
        },
        set(tricksControl) {
            _tricksControl = tricksControl;
            resolve(tricksControl);
        },
        configurable: true,
        enumerable: true,
    });
});

const waitForMmtAddContextFn = new Promise<MinimapTricksAddContextFn>(
    (resolve) => {
        if (unsafeWindow._MMT_addContext) {
            resolve(unsafeWindow._MMT_addContext);
            return;
        }

        let _contexts: MinimapTricksAddContextFn;
        Object.defineProperty(unsafeWindow, "_MMT_addContext", {
            get() {
                return _contexts;
            },
            set(contexts) {
                _contexts = contexts;
                resolve(contexts);
            },
            configurable: true,
            enumerable: true,
        });
    }
);

export function addMarkerContextMenuListener(
    marker: maplibregl.Marker,
    contextName: MinimapTricksContext
) {
    marker.getElement().addEventListener("contextmenu", (event) => {
        openContextMenu(event, marker.getLngLat(), contextName);
    });
}

function openContextMenu(
    event: MouseEvent,
    pos: maplibregl.LngLat,
    contextName: MinimapTricksContext
) {
    if (!tricksControl) return;

    event.stopPropagation();
    event.preventDefault();

    const data = {};
    tricksControl.openMenu(
        contextName,
        pos.lat,
        pos.lng,
        event.clientX,
        event.clientY,
        data
    );
}
