import { currentData, getDestinationString, refreshPath } from ".";
import { LOG_PREFIX } from "./constants";
import { rerenderCompletePathSegments } from "./map/lines";
import { rerenderStopMarkers } from "./map/markers";
import { calculateDistance } from "./math";
import { newPosition, parseCoordinatesString } from "./pos";

/**
 * Returns the list of destinations for the path we're following. Usually this will just be the one
 * destination, but can include more if the path has stops in it.
 */
export function getPathDestinations(): GeoJSON.Position[] {
    const remainingStops = getUnorderedStops();
    const lastDestination = parseCoordinatesString(getDestinationString());
    if (!currentData || !lastDestination) return [];

    const stops = [];

    // find the closest stop in remainingStops
    let currentPosition = newPosition(currentData.lat, currentData.lng);
    while (remainingStops.length > 0) {
        let closestIndex = -1;
        let closestDistance = Number.POSITIVE_INFINITY;
        for (const [candidateIndex, candidate] of remainingStops.entries()) {
            const candidateDistance = calculateDistance(
                currentPosition,
                candidate
            );
            if (candidateDistance < closestDistance) {
                closestIndex = candidateIndex;
                closestDistance = candidateDistance;
            }
        }

        const closestStop = remainingStops[closestIndex];
        if (!closestStop) {
            throw Error(
                `closestStop should\'ve been set. closestIndex: ${closestIndex}, remainingStops: ${remainingStops}`
            );
        }

        stops.push(closestStop);
        remainingStops.splice(closestIndex, 1);
    }

    stops.push(lastDestination);

    return stops;
}

export function addStopToPath(pos: GeoJSON.Position) {
    const currentStops = getUnorderedStops();
    if (currentStops.find((s) => s[0] === pos[0] && s[1] === pos[1])) {
        // stop is already present
        return;
    }
    currentStops.push(pos);
    setStops(currentStops);
    rerenderCompletePathSegments();
    rerenderStopMarkers();
    refreshPath();
}
export function removeStop(pos: GeoJSON.Position): boolean {
    const oldStops = getUnorderedStops();
    const newStops = oldStops.filter((s) => s[0] !== pos[0] || s[1] !== pos[1]);
    if (newStops.length === oldStops.length) {
        console.warn(
            LOG_PREFIX,
            "failed to remove stop at",
            pos,
            "currentStops:",
            oldStops
        );
        return false;
    }

    setStops(newStops);
    rerenderCompletePathSegments();
    rerenderStopMarkers();
    refreshPath();

    return true;
}

/**
 * @returns {GeoJSON.Position[]} array of [lng, lat]
 */
export function getUnorderedStops(): GeoJSON.Position[] {
    return JSON.parse(GM_getValue("stops") || "[]");
}
export function setStops(stops: GeoJSON.Position[]) {
    GM_setValue("stops", JSON.stringify(stops));
}
