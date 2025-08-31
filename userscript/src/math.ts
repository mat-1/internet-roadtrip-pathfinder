import { getLat, getLng } from "./pos";

/**
 * in meters, from Google Maps
 */
const EARTH_RADIUS = 6_378_137;
/**
 * @param origin lat, lng
 * @param dest lat, lng
 * @returns in meters
 */
export function calculateDistance(origin: GeoJSON.Position, dest: GeoJSON.Position): number {
    const aLat = getLat(origin);
    const bLat = getLat(dest);

    const aLng = getLng(origin);
    const bLng = getLng(dest);

    const theta1 = toRadians(aLat);
    const theta2 = toRadians(bLat);
    const deltaTheta = toRadians(bLat - aLat);
    const deltaLambda = toRadians(bLng - aLng);

    const a =
        Math.pow(Math.sin(deltaTheta / 2), 2) +
        Math.cos(theta1) *
        Math.cos(theta2) *
        Math.pow(Math.sin(deltaLambda / 2), 2);
    const c = 2 * Math.asin(Math.sqrt(a));
    return EARTH_RADIUS * c;
}
/**
 * @param origin lat, lng
 * @param dest lat, lng
 * @returns in degrees
 */
export function calculateHeading(origin: GeoJSON.Position, dest: GeoJSON.Position): number {
    const [aLng, aLat] = [toRadians(getLng(origin)), toRadians(getLat(origin))];
    const [bLng, bLat] = [toRadians(getLng(dest)), toRadians(getLat(dest))];
    const deltaLng = bLng - aLng;

    const [aLatSin, aLatCos] = [Math.sin(aLat), Math.cos(aLat)];
    const [bLatSin, bLatCos] = [Math.sin(bLat), Math.cos(bLat)];
    const [deltaLngSin, deltaLngCos] = [Math.sin(deltaLng), Math.cos(deltaLng)];

    const s = deltaLngSin * bLatCos;
    const c = aLatCos * bLatSin - aLatSin * bLatCos * deltaLngCos;

    return (toDegrees(Math.atan2(s, c)) + 360) % 360;
}
/**
 *
 * @param a in degrees
 * @param in degrees
 * @returns in degrees, between 0 and 360
 */
export function calculateHeadingDiff(a: number, b: number): number {
    a = (a + 360) % 360;
    b = (b + 360) % 360;

    let diff = Math.abs(a - b);
    if (diff > 180) {
        diff = 360 - diff;
    }
    return diff;
}

export function toRadians(degrees: number) {
    return degrees * (Math.PI / 180);
}
export function toDegrees(radians: number) {
    return radians * (180 / Math.PI);
}


