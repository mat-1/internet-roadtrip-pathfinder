// geojson uses [lng,lat] and we mostly do the same, so we handle all of that here to avoid mistakes

export function getLat(pos: GeoJSON.Position): number {
    return pos[1]!;
}
export function getLng(pos: GeoJSON.Position): number {
    return pos[0]!;
}
export function newPosition(lat: number, lng: number): GeoJSON.Position {
    return [lng, lat];
}

/**
 * @param coords formatted like "lat,lng"
 * @returns [lng, lat]
 */
export function parseCoordinatesString(
    coords: string
): GeoJSON.Position | null {
    if (!coords.includes(",")) return null;
    const [lat, lng] = coords.split(",").map(Number);
    if (lat === undefined || lng === undefined) return null;
    return newPosition(lat, lng);
}
