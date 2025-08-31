import { map } from ".";
import { updateLastCostRecalculationTimestamp } from "..";
import { calculatingPathId, clearCalculatingPathId } from "../api";
import { LOG_PREFIX } from "../constants";
import { findClosestPanoInPath } from "../options";
import { getLat, getLng } from "../pos";
import { SETTINGS } from "../settings";
import { getPathDestinations, getUnorderedStops } from "../stops";
import { sleep } from "../utils";

/**
 * The string IDs for the path on the map that's currently being calculated.
 */
type CalculatingPathSourceId = "best_path" | "current_searching_path";

/**
 * The string IDs that we use for all lines that we render on the maplibre map.
 * Superset of `CalculatingPathSourceId`.
 */
type AnyPathSourceId = CalculatingPathSourceId | "best_path_segments";

interface PathSegmentMetadata {
    source: {
        pos: GeoJSON.Position;
        heading: number;
    };
    destination: GeoJSON.Position;
}

/**
 * A map of path IDs to the best_paths that we calculated to completion.
 */
export const completePathSegments = new Map<number, GeoJSON.Position[]>();
/**
 * Path ID to its cost (which is the duration in seconds).
 */
export const pathSegmentCosts = new Map<number, number>();

export const pathIdsToPathSegmentMetadata = new Map<
    number,
    PathSegmentMetadata
>();
/**
 * `lat,lng` to the path id, this includes destinations that aren't currently being used
 */
export const destinationToPathIdMap = new Map<`${number},${number}`, number>();
/**
 * A map of path IDs to their expected destinations. It'll likely be a few meters from the actual destination.
 */
export const pathIdToDestination = new Map<number, GeoJSON.Position>();

export const calculatingPaths = new Map<
    CalculatingPathSourceId,
    GeoJSON.Position[]
>();

export async function setupPathSources() {
    console.debug(LOG_PREFIX, "waiting for old-route to render");
    let waitedCount = 0;
    // alternatively just wait 2 seconds, in case internet-roadtrip.neal.fun/route is broken
    while (map.getSource("old-route") === undefined && waitedCount < 20) {
        await sleep(100);
        waitedCount += 1;
    }

    console.debug(LOG_PREFIX, "setting up path sources");
    setupPathSource("current_searching_path", "#00f");
    setupPathSource("best_path", "#f0f");
    setupPathSource("best_path_segments", "#f0f");
}
function setupPathSource(pathSourceId: AnyPathSourceId, color: string) {
    map.addSource(pathSourceId, {
        type: "geojson",
        data: {
            type: "Feature",
            properties: {},
            geometry: {
                type: "LineString",
                coordinates: [],
            },
        },
    });
    map.addLayer({
        id: pathSourceId,
        type: "line",
        source: pathSourceId,
        layout: {
            "line-join": "round",
            "line-cap": "round",
        },
        paint: {
            "line-color": color,
            "line-width": 4,
        },
    });
}
export async function updatePathSource(
    pathSourceId: CalculatingPathSourceId,
    keepPrefixLength: number,
    append: GeoJSON.Position[]
) {
    let curPath = calculatingPaths.get(pathSourceId) ?? [];
    curPath = curPath.slice(0, keepPrefixLength);
    curPath.push(...append);
    calculatingPaths.set(pathSourceId, curPath);

    rerenderPath(pathSourceId);
}

export function updateCurrentLocation(curPos: GeoJSON.Position): boolean {
    // check if the new location is near the front of our pathfinder's best path

    const firstPath = getFirstPath().slice(0, 10);

    const [closestPanoInBestPathIndex, closestPanoInBestPathDistance] =
        findClosestPanoInPath(curPos, firstPath);

    if (closestPanoInBestPathIndex === -1) {
        // this is usually fine, but can sometimes happen if we were stuck and got teleported out
        return false;
    }

    if (closestPanoInBestPathDistance > 20) {
        return false;
    }

    const i = closestPanoInBestPathIndex;
    panosAdvancedInFirstPath += i;
    updateLastCostRecalculationTimestamp();
    console.debug(
        LOG_PREFIX,
        "close enough, updating panosAdvancedInBestPath to",
        panosAdvancedInFirstPath
    );
    rerenderPath("best_path");
    rerenderPath("current_searching_path");
    rerenderCompletePathSegments();

    return true;
}

export function getFirstPath(): GeoJSON.Position[] {
    const firstPathDest = getPathDestinations()[0]!;
    const firstPathId = convertDestinationToPathId(firstPathDest);
    if (firstPathId === undefined) {
        console.debug(
            LOG_PREFIX,
            "called updateCurrentLocation before the current path was requested"
        );
        return [];
    }

    const unskipped =
        calculatingPathId === firstPathId
            ? calculatingPaths.get("best_path")
            : completePathSegments.get(firstPathId);
    return unskipped?.slice(panosAdvancedInFirstPath) ?? [];
}

export let panosAdvancedInFirstPath = 0;
export function rerenderPath(pathSourceId: CalculatingPathSourceId) {
    let skip = 0;
    if (calculatingPathId !== undefined) {
        const pathDestination =
            pathIdsToPathSegmentMetadata.get(calculatingPathId)!.destination;
        const paths = getPathDestinations();
        const firstDestInPath = paths[0]!;
        if (
            firstDestInPath[0] === pathDestination[0] &&
            firstDestInPath[1] === pathDestination[1]
        ) {
            // we've confirmed that this is the first path, so skip some panos
            skip = panosAdvancedInFirstPath;
        }
    }

    console.debug(LOG_PREFIX, "rendering", pathSourceId, "and skipping", skip);
    let path = calculatingPaths.get(pathSourceId)?.slice(skip) ?? [];

    // hide the current_path if the setting is checked
    // hide the current_searching_path if the setting is checked
    if (
        pathSourceId === "current_searching_path" &&
        !SETTINGS.current_searching_path
    ) {
        path = [];
    }

    const pathSource: maplibregl.GeoJSONSource | undefined =
        map.getSource(pathSourceId);

    pathSource?.setData({
        type: "Feature",
        properties: {},
        geometry: {
            type: "LineString",
            coordinates: path,
        },
    });
}

export function rerenderCompletePathSegments() {
    const multiLines: GeoJSON.Position[][] = [];
    for (const [index, stopDest] of getPathDestinations().entries()) {
        const pathId = convertDestinationToPathId(stopDest);
        if (pathId === undefined) {
            console.debug(
                LOG_PREFIX,
                "failed rendering path segment because it hasn't been requested yet:",
                destinationToPathIdMap,
                stopDest
            );
            continue;
        }

        let skip = 0;
        if (index === 0) {
            skip = panosAdvancedInFirstPath;
        }
        console.debug(
            LOG_PREFIX,
            "rendering best_path_segments and skipping",
            skip
        );

        const lines = completePathSegments.get(pathId)?.slice(skip);

        if (lines) {
            multiLines.push(lines);
        } else {
            console.warn(
                LOG_PREFIX,
                "stop destination",
                stopDest,
                "not present in completePathSegments. this probably just means that we haven't finished calculating it"
            );
            break;
        }
    }

    const pathSource: maplibregl.GeoJSONSource =
        map.getSource("best_path_segments")!;
    pathSource.setData({
        type: "Feature",
        properties: {},
        geometry: {
            type: "MultiLineString",
            coordinates: multiLines,
        },
    });
}

/**
 * Remove the pathfinder's lines from the map and forget their current state. You should probably
 * use `clearAllPaths` instead.
 */
export function resetRenderedPath() {
    panosAdvancedInFirstPath = 0;
    clearCalculatingPaths();
    rerenderCompletePathSegments();
    rerenderPath("best_path");
    rerenderPath("current_searching_path");
}

export function updateCompletePathSegment(
    pathId: number,
    path: GeoJSON.Position[],
    cost: number
) {
    completePathSegments.set(pathId, path);
    pathSegmentCosts.set(pathId, cost);

    // no path is being calculated at this point anymore
    clearCalculatingPathId();
    // the path is already in completePathSegments, so remove it from
    // calculatingPaths to make it so we don't render the same path twice
    calculatingPaths.clear();
}

export function convertDestinationToPathId(
    pos: GeoJSON.Position
): number | undefined {
    return destinationToPathIdMap.get(`${getLat(pos)},${getLng(pos)}`);
}

export function clearCalculatingPaths() {
    if (getUnorderedStops().length === 0) {
        // persist these so we can avoid recalculating paths with many stops

        completePathSegments.clear();
        pathSegmentCosts.clear();
        pathIdsToPathSegmentMetadata.clear();
        destinationToPathIdMap.clear();
        pathIdToDestination.clear();
    }

    calculatingPaths.clear();
    clearCalculatingPathId();
}

export function clearCachedPaths() {
    completePathSegments.clear();
    pathSegmentCosts.clear();
    pathIdsToPathSegmentMetadata.clear();
    destinationToPathIdMap.clear();
    pathIdToDestination.clear();
    calculatingPaths.clear();
    clearCalculatingPathId();
}
