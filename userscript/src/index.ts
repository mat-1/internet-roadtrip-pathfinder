import "./meta.js?userscript-metadata";

import { getLat, getLng, newPosition, parseCoordinatesString } from "./pos";
import * as api from "./api";
import { initSettingsTab, SETTINGS } from "./settings";
import { LOG_PREFIX } from "./constants";
import { clearOptionHighlights, showBestOption } from "./options";
import {
    completePathSegments,
    calculatingPaths,
    pathIdToDestination,
    pathSegmentCosts,
    rerenderCompletePathSegments,
    resetRenderedPath,
    setupPathSources,
    updateCompletePathSegment,
    updateCurrentLocation,
    updatePathSource,
    convertDestinationToPathId,
    panosAdvancedInFirstPath,
    clearCalculatingPaths,
} from "./map/lines";
import { prettyTime, sleep } from "./utils";
import {
    initMarkers,
    rerenderStopMarkers,
    updateDestinationMarker,
} from "./map/markers";
import { initMap, map } from "./map";
import {
    getPathDestinations,
    getUnorderedStops,
    removeStop,
    setStops,
} from "./stops";
import { calculateDistance, calculateHeading } from "./math";
import { tryInitMmt } from "./mmt";

export let pathfinderInfoEl: HTMLSpanElement;
let pathfinderRefreshBtnEl: HTMLButtonElement;
let destinationInputEl: HTMLInputElement;

export let lastCostRecalculationTimestamp = Date.now();

async function init() {
    const vdomContainer = await IRF.vdom.container;
    await initMap();

    injectStylesheet();

    const pathfinderContainerEl = document.createElement("div");
    pathfinderContainerEl.id = "pathfinder-container";

    destinationInputEl = document.createElement("input");
    destinationInputEl.classList.add("pathfinder-destination-input");
    destinationInputEl.placeholder = "lat, lng";
    destinationInputEl.value = getDestinationString();

    pathfinderRefreshBtnEl = document.createElement("button");
    pathfinderRefreshBtnEl.textContent = "🗘";
    pathfinderRefreshBtnEl.classList.add("pathfinder-refresh-btn");
    pathfinderRefreshBtnEl.disabled = true;

    pathfinderInfoEl = document.createElement("span");
    pathfinderInfoEl.classList.add("pathfinder-info");
    setInterval(() => {
        let totalCost = 0;
        let totalPanos = 0;
        for (const dest of getPathDestinations()) {
            const pathSegmentId = convertDestinationToPathId(dest);
            if (pathSegmentId === undefined) {
                // means we haven't calculated this path yet, so we can't set an eta!
                return;
            }
            const pathSegmentCost = pathSegmentCosts.get(pathSegmentId);
            if (pathSegmentCost === undefined) {
                // means the path hasn't finished being calculated
                return;
            }

            totalCost += pathSegmentCost;
            const actualPath = completePathSegments.get(pathSegmentId)!;
            totalPanos += actualPath.length;
        }

        if (totalCost) {
            const secondsSinceLastCostRecalculation =
                (Date.now() - lastCostRecalculationTimestamp) / 1000;
            totalCost -= secondsSinceLastCostRecalculation;

            const advancedPercentage = panosAdvancedInFirstPath / totalPanos;
            const adjustedCost = totalCost * (1 - advancedPercentage);

            const prettyEta = prettyTime(adjustedCost);
            pathfinderInfoEl.textContent = `ETA: ${prettyEta}`;
        }
    }, 1000);

    pathfinderContainerEl.appendChild(destinationInputEl);
    pathfinderContainerEl.appendChild(pathfinderRefreshBtnEl);
    pathfinderContainerEl.appendChild(pathfinderInfoEl);

    vdomContainer.state.updateData = new Proxy(vdomContainer.state.updateData, {
        apply(oldUpdateData, thisArg, args) {
            // onUpdateData is a promise so errors won't propagate to here
            onUpdateData(args[0]);

            return oldUpdateData.apply(thisArg, args);
        },
    });

    const mapContainerEl = map.getContainer().parentElement!;
    mapContainerEl.appendChild(pathfinderContainerEl);

    await initMarkers();

    destinationInputEl.addEventListener("change", () => {
        console.debug(LOG_PREFIX, "destination input changed");
        refreshPath();
    });
    pathfinderRefreshBtnEl.addEventListener("click", () => {
        console.debug(LOG_PREFIX, "refresh button clicked");
        refreshPath();
    });

    tryInitMmt();

    initSettingsTab();
    // this has to be done after settings are loaded so the backend url is correct
    api.connect();

    await setupPathSources();
    // wait until the websocket is open
    await api.waitUntilConnected();
    // now wait until we've received data from the internet roadtrip ws
    while (!currentData) {
        await sleep(100);
    }

    console.debug(LOG_PREFIX, "start called");
    refreshPath();
}

export function updateLastCostRecalculationTimestamp() {
    lastCostRecalculationTimestamp = Date.now();
}

/** called when we receive a progress update from the pathfinder server */
export function onProgress(data: api.PathfinderMessage) {
    pathfinderRefreshBtnEl.disabled = false;

    if (data.percent_done < 0) {
        // means the path was cleared
        clearAllPaths();
        rerenderCompletePathSegments();
        return;
    }

    updatePathSource(
        "best_path",
        data.best_path_keep_prefix_length,
        data.best_path_append
    );
    updatePathSource(
        "current_searching_path",
        data.current_path_keep_prefix_length,
        data.current_path_append
    );

    if (data.percent_done < 1) {
        // round to 5 decimal places but truncate to 1
        const percentDoneString = (data.percent_done * 100)
            .toFixed(5)
            .match(/^-?\d+(?:\.\d{0,1})?/)![0];
        pathfinderInfoEl.textContent = `${percentDoneString}%`;
        return;
    }

    // path is done

    const pathId = data.id;
    updateCompletePathSegment(
        pathId,
        calculatingPaths.get("best_path")!,
        data.best_path_cost
    );
    console.debug(
        LOG_PREFIX,
        `finished path ${pathId}, updated in completePathSegments`
    );
    rerenderCompletePathSegments();
    updateLastCostRecalculationTimestamp();

    // find the next segment if possible!

    const lastPosition = completePathSegments.get(pathId)!.at(-1)!;
    const secondLastPosition = completePathSegments.get(pathId)!.at(-2)!;
    const lastHeading = calculateHeading(secondLastPosition, lastPosition);
    const expectedDestination = pathIdToDestination.get(pathId)!;
    const allDestinations = getPathDestinations();
    const destinationIndex = allDestinations.findIndex(
        (d) =>
            d[0] === expectedDestination[0] && d[1] === expectedDestination[1]
    );

    console.debug(
        LOG_PREFIX,
        "allDestinations:",
        allDestinations,
        "expectedDestination:",
        expectedDestination,
        "destinationIndex",
        destinationIndex
    );

    if (destinationIndex === -1) {
        console.warn(
            LOG_PREFIX,
            "the path we just found wasn't in getPathDestinations():",
            allDestinations,
            "expectedDestination:",
            expectedDestination
        );
        return;
    }
    if (destinationIndex === allDestinations.length - 1) {
        console.debug(LOG_PREFIX, "found last segment in path");
        return;
    }

    const nextDestination = allDestinations[destinationIndex + 1]!;

    api.requestNewPath(
        lastHeading,
        lastPosition,
        nextDestination,
        // pathfinder server doesn't send us pano ids
        undefined
    );
    rerenderCompletePathSegments();
}

/**
 * Send a message to stop calculating a path, and remove the lines from the map.
 */
function abortPathfinding() {
    // note that this will get set again when the server sends us a progress update with percent_done being -1
    if (pathfinderInfoEl) pathfinderInfoEl.textContent = "";

    if (api.calculatingPathId !== undefined) {
        api.sendWebSocketMessage({
            kind: "abort",
            id: api.calculatingPathId,
        });
    }
    clearAllPaths();
}

/**
 * Remove the pathfinder's lines from the map, without aborting the current path.
 */
export function clearAllPaths() {
    pathfinderInfoEl.textContent = "";
    resetRenderedPath();
}

export function updateDestinationFromString(destString: string) {
    setDestinationString(destString);
    const dest = parseCoordinatesString(destString);
    updateDestinationMarker(dest);
    if (!dest) {
        document.body.classList.remove("pathfinder-has-destination");
        setStops([]);
        // abortPathfinding has to happen before clearAllPaths so the calculatingPathId is still set
        abortPathfinding();
        clearCalculatingPaths();
        rerenderCompletePathSegments();
        rerenderStopMarkers();
        clearAllPaths();

        return;
    }
    clearAllPaths();
    document.body.classList.add("pathfinder-has-destination");

    if (!currentData) {
        // if we haven't received any data from the game yet then we can't know our current location
        return;
    }

    const curPos = newPosition(currentData.lat, currentData.lng);
    // at least one destination must be present because we just set the destination string and
    // checked that it was valid
    const firstDestination = getPathDestinations()[0]!;
    api.requestNewPath(
        currentData.heading,
        curPos,
        firstDestination,
        currentData.pano
    );
}

export let previousData: RoadtripMessage | null = null;
export let currentData: RoadtripMessage | null = null;

/**
 * The number of times in a row that the current position wasn't found in the best path.
 * This exists so we can recalculate the path if this value gets too high.
 */
let lostPathCount = 0;

/**
 * Called whenever we receive a message from the game WebSocket.
 */
async function onUpdateData(msg: RoadtripMessage) {
    [previousData, currentData] = [currentData, msg];

    const curPos = newPosition(currentData.lat, currentData.lng);

    const locationChanged =
        previousData?.lat !== getLat(curPos) ||
        previousData?.lng !== getLng(curPos) ||
        previousData?.heading !== currentData.heading;

    if (locationChanged) clearOptionHighlights();

    if (SETTINGS.remove_reached_stops) {
        const remainingStops = getUnorderedStops();
        for (const stop of [...remainingStops]) {
            if (calculateDistance(stop, curPos) < 15 /* meters */) {
                removeStop(stop);
                break;
            }
        }
    }

    if (locationChanged && getPathDestinations().length > 0) {
        const isPathFound = updateCurrentLocation(curPos);
        if (isPathFound) {
            lostPathCount = 0;

            // wait a bit to make sure that any new elements are created
            await sleep(1100);
            showBestOption();
        } else {
            console.warn(LOG_PREFIX, `lost path? (#${lostPathCount})`);
            lostPathCount += 1;
            if (lostPathCount >= 3) {
                lostPathCount = 0;
                refreshPath();
            }
        }
    }
}

export async function refreshPath() {
    pathfinderRefreshBtnEl.disabled = true;

    const destinationValue = getDestinationString();
    const hasDestination = destinationValue.trim() !== "";
    document.body.classList.toggle(
        "pathfinder-has-destination",
        hasDestination
    );
    if (!hasDestination) {
        updateDestinationMarker(null);
        abortPathfinding();
        return;
    }

    updateDestinationFromString(destinationValue);

    // makes it so we don't wait until the next location change to highlight the new best option
    await sleep(2000);
    showBestOption();
}

/**
 * @returns A string that should be formatted as `lat,lng`, but might not be.
 */
export function getDestinationString(): string {
    return GM_getValue("destination") ?? "";
}
/**
 * @param value A string that should be formatted as `lat,lng`, but might not be.
 */
function setDestinationString(value: string) {
    GM_setValue("destination", value.trim());
}

function injectStylesheet() {
    GM_addStyle(`
    body:not(.pathfinder-found-minimap-tricks) {
      & .map-container .info-button {
        /* overlaps with our ui */
        display: none;
      }
      & .pathfinder-refresh-btn {
        line-height: 1;
        padding: 0.2em;
      }
      & .pathfinder-info {
        background-color: #fff;
        padding: 0.1em 0.3em;
      }
    }

    body.pathfinder-found-minimap-tricks {
      & #pathfinder-container {
        display: none;
      }

      & .pathfinder-info {
        margin-right: 36px;

        &:empty {
          display: none;
        }
      }
    }

    .pathfinder-destination-marker {
      width: 25px;
      cursor: default;
    }
    .pathfinder-stop-marker {
      width: 15px;
      cursor: default;
    }
    .pathfinder-chosen-option path {
      fill: #f0f !important;
    }

    body:not(.pathfinder-has-destination) {
      & .pathfinder-clear-path-mmt-side-button,
      .pathfinder-clear-path-mmt-context-menu-button,
      .pathfinder-add-stop-mmt-context-menu-button {
        display: none !important;
      }
    }
  `);
}

export function replacePathfinderInfoEl(newEl: HTMLElement) {
    pathfinderInfoEl.remove();
    pathfinderInfoEl = newEl;
}

init();
