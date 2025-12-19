import { map } from ".";
import { addMarkerContextMenuListener, tricksControl } from "../mmt";
import { getLat, getLng } from "../pos";
import { getOrderedStops, reorderStop } from "../stops";
import { SETTINGS } from "../settings";

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

export async function newStopMarker(index: number): Promise<maplibregl.Marker> {
    const maplibre = await IRF.modules.maplibre;
    const markerEl = document.createElement("div");
    markerEl.className = "pathfinder-stop-marker";
    
    const imgEl = document.createElement("img");
    imgEl.src = GM_getResourceURL("flagCheckerboardPng");
    markerEl.appendChild(imgEl);
    
    if (SETTINGS.show_stops_menu) {
        const numberEl = document.createElement("span");
        numberEl.className = "pathfinder-stop-number";
        numberEl.textContent = String(index + 1);
        markerEl.appendChild(numberEl);
    }
    
    const marker = new maplibre.Marker({
        element: markerEl,
        anchor: "bottom-left",
    });
    addMarkerContextMenuListener(marker, "Pathfinder stop");

    return marker;
}

export async function rerenderStopMarkers() {
    const orderedStops = getOrderedStops();

    for (const marker of stopMarkers) {
        marker.remove();
    }
    stopMarkers = [];
    
    for (let i = 0; i < orderedStops.length; i++) {
        const stopMarker = await newStopMarker(i);
        const stopPos = orderedStops[i]!;
        stopMarker.setLngLat(stopPos as [number, number]).addTo(map);
        stopMarkers.push(stopMarker);
    }
}
