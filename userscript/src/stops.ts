import { currentData, getDestinationString, refreshPath } from ".";
import { LOG_PREFIX } from "./constants";
import { rerenderCompletePathSegments } from "./map/lines";
import { rerenderStopMarkers } from "./map/markers";
import { calculateDistance } from "./math";
import { newPosition, parseCoordinatesString } from "./pos";
import { rerenderStopsMenu } from "./stops-menu";

/**
 * Returns the list of destinations for the path we're following. Usually this will just be the one
 * destination, but can include more if the path has stops in it.
 */
export function getPathDestinations(): GeoJSON.Position[] {
    const orderedStops = getOrderedStops();
    const lastDestination = parseCoordinatesString(getDestinationString());
    if (!lastDestination) return [];

    return [...orderedStops, lastDestination];
}

export function addStopToPath(pos: GeoJSON.Position) {
    const currentStops = getOrderedStops();
    if (currentStops.find((s) => s[0] === pos[0] && s[1] === pos[1])) {
        // stop is already present
        return;
    }
    
    if (!currentData) {
        currentStops.push(pos);
    } else {
        const currentPosition = newPosition(currentData.lat, currentData.lng);
        const finalDestination = parseCoordinatesString(getDestinationString());
        
        const pathPositions: GeoJSON.Position[] = [currentPosition, ...currentStops];
        if (finalDestination) {
            pathPositions.push(finalDestination);
        }
        
        let bestInsertIndex = currentStops.length;
        let bestDetour = Infinity;
        
        for (let i = 0; i < pathPositions.length - 1; i++) {
            const from = pathPositions[i]!;
            const to = pathPositions[i + 1]!;
            
            const originalDist = calculateDistance(from, to);
            const detourDist = calculateDistance(from, pos) + calculateDistance(pos, to) - originalDist;
            
            if (detourDist < bestDetour) {
                bestDetour = detourDist;
                bestInsertIndex = i;
            }
        }
        
        currentStops.splice(bestInsertIndex, 0, pos);
    }
    
    setStops(currentStops);
    rerenderCompletePathSegments();
    rerenderStopMarkers();
    rerenderStopsMenu();
    refreshPath();
}
export function removeStop(pos: GeoJSON.Position): boolean {
    const oldStops = getOrderedStops();
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
    rerenderStopsMenu();
    refreshPath();

    return true;
}

/**
 * @returns {GeoJSON.Position[]} array of [lng, lat] in order
 */
export function getOrderedStops(): GeoJSON.Position[] {
    return JSON.parse(GM_getValue("stops") || "[]");
}

export function setStops(stops: GeoJSON.Position[]) {
    GM_setValue("stops", JSON.stringify(stops));
}

/**
 * Move a stop from one index to another
 */
export function reorderStop(fromIndex: number, toIndex: number) {
    const stops = getOrderedStops();
    if (fromIndex < 0 || fromIndex >= stops.length || toIndex < 0 || toIndex >= stops.length) {
        return;
    }
    const [movedStop] = stops.splice(fromIndex, 1);
    if (!movedStop) return;
    stops.splice(toIndex, 0, movedStop);
    setStops(stops);
    rerenderCompletePathSegments();
    rerenderStopMarkers();
    rerenderStopsMenu();
    refreshPath();
}
