import { map } from ".";
import { addMarkerContextMenuListener, tricksControl } from "../mmt";
import { getLat, getLng } from "../pos";
import { getUnorderedStops } from "../stops";

export let destinationMarker: maplibregl.Marker;

/**
 * should be the same length as the number of stops
 */
let stopMarkers: maplibregl.Marker[] = [];

export async function initMarkers() {
    const maplibre = await IRF.modules.maplibre;
    destinationMarker = new maplibre.Marker({
        element: (() => {
            const imgEl = document.createElement("img");
            imgEl.className = "pathfinder-destination-marker";
            imgEl.src = GM_getResourceURL("flagCheckerboardPng");
            return imgEl;
        })(),
        anchor: "bottom-left",
    });

    addMarkerContextMenuListener(destinationMarker, "Pathfinder destination");

    rerenderStopMarkers();
}

export function updateDestinationMarker(position: GeoJSON.Position | null) {
    if (!position) {
        // if no coords are passed then the point is removed
        destinationMarker.remove();
        return;
    }
    destinationMarker
        .setLngLat([getLng(position), getLat(position)])
        .addTo(map);
}

export async function newStopMarker(): Promise<maplibregl.Marker> {
    const maplibre = await IRF.modules.maplibre;
    const marker = new maplibre.Marker({
        element: (() => {
            const imgEl = document.createElement("img");
            imgEl.className = "pathfinder-stop-marker";
            imgEl.src = GM_getResourceURL("flagCheckerboardPng");
            return imgEl;
        })(),
        anchor: "bottom-left",
    });
    addMarkerContextMenuListener(marker, "Pathfinder stop");

    return marker;
}

export async function rerenderStopMarkers() {
    const unorderedStops = getUnorderedStops();

    while (stopMarkers.length > unorderedStops.length) {
        stopMarkers.pop()!.remove();
    }
    while (unorderedStops.length > stopMarkers.length) {
        const stopMarker = await newStopMarker();
        // coordinates are required, but they'll get updated in a moment
        stopMarker.setLngLat([0, 0]).addTo(map);
        stopMarkers.push(stopMarker);
    }
    // stopMarkers and unorderedStops are now the same length, now update the lat/lng for all of them
    for (let i = 0; i < stopMarkers.length; i++) {
        const marker = stopMarkers[i]!;
        const stopPos = unorderedStops[i]!;
        marker.setLngLat([getLng(stopPos), getLat(stopPos)]);
    }
}
