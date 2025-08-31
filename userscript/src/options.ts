/**
 * Code related to picking the best option and finding panoramas in the pathfinder's path.
 */

import { currentData } from ".";
import { LOG_PREFIX } from "./constants";
import {
    calculatingPaths,
    getFirstPath,
    panosAdvancedInFirstPath,
} from "./map/lines";
import {
    calculateDistance,
    calculateHeading,
    calculateHeadingDiff,
} from "./math";
import { newPosition } from "./pos";

export function showBestOption() {
    if (!currentData) {
        throw Error("called showBestOption when currentData was still null");
    }

    const currentPos = newPosition(currentData.lat, currentData.lng);

    const firstPath = getFirstPath();
    if (firstPath.length < panosAdvancedInFirstPath + 2) return;

    const bestNextPos = firstPath[panosAdvancedInFirstPath + 1]!;

    const bestHeading = calculateHeading(currentPos, bestNextPos);
    console.debug(LOG_PREFIX, "option bestHeading", bestHeading);

    let bestOptionIndex = -1;
    let bestOptionHeadingDiff = Infinity;

    const options = currentData.options;

    // first, check only the option that have lat+lng present (since those are more reliable)
    for (let optionIndex = 0; optionIndex < options.length; optionIndex++) {
        const option = options[optionIndex]!;
        if (!option.lat || !option.lng) continue;
        const optionPos = newPosition(option.lat, option.lng);

        const firstPathSliced = firstPath.slice(
            panosAdvancedInFirstPath,
            panosAdvancedInFirstPath + 2
        );
        const [matchingPanoInPathIndex, matchingPanoInPathDistance] =
            findClosestPanoInPath(optionPos, firstPathSliced);
        console.debug(
            LOG_PREFIX,
            "option with lat+lng",
            firstPathSliced[matchingPanoInPathIndex],
            matchingPanoInPathDistance
        );

        // heading diff and distance in meters aren't really comparable, but if a pano had a distance
        // of less than 1m then it's almost guaranteed to be the one we want anyways.
        if (
            matchingPanoInPathDistance < 1 &&
            matchingPanoInPathDistance < bestOptionHeadingDiff
        ) {
            bestOptionIndex = optionIndex;
            bestOptionHeadingDiff = matchingPanoInPathDistance;
        }
    }

    // if nothing was found from the lat+lng check, do the less reliable heading check instead
    if (bestOptionIndex < 0) {
        for (let optionIndex = 0; optionIndex < options.length; optionIndex++) {
            const option = options[optionIndex]!;

            const optionHeading = option.heading;
            const optionHeadingDiff = calculateHeadingDiff(
                optionHeading,
                bestHeading
            );
            if (optionHeadingDiff < bestOptionHeadingDiff) {
                bestOptionIndex = optionIndex;
                bestOptionHeadingDiff = optionHeadingDiff;
            }
        }
    }

    if (bestOptionHeadingDiff > 100) {
        console.warn(LOG_PREFIX, "all of the options are bad!");
    } else {
        console.debug(
            LOG_PREFIX,
            "best option is",
            options[bestOptionIndex],
            `(diff: ${bestOptionHeadingDiff})`
        );

        highlightOptionIndex(bestOptionIndex);
    }
}

function highlightOptionIndex(optionIndex: number) {
    const optionArrowEls = Array.from(document.querySelectorAll(".option"));

    for (let i = 0; i < optionArrowEls.length; i++) {
        const optionArrowEl = optionArrowEls[i]!;
        if (i === optionIndex)
            optionArrowEl.classList.add("pathfinder-chosen-option");
        else optionArrowEl.classList.remove("pathfinder-chosen-option");
    }
}
export function clearOptionHighlights() {
    for (const optionArrowEl of document.querySelectorAll(".option")) {
        optionArrowEl.classList.remove("pathfinder-chosen-option");
    }
}

/**
 * @returns The index of the closest pano in the path, and its distance.
 * Also, panos after the first one with a heading difference greater than 100 degrees are ignored.
 */
export function findClosestPanoInPath(
    targetPos: GeoJSON.Position,
    path: GeoJSON.Position[]
): [number, number] {
    let closestPanoInFirstPathIndex = -1;
    let closestPanoInFirstPathDistance = Infinity;

    for (let i = 0; i < path.length; i++) {
        const candidatePos = path[i]!;
        const distanceToCur = calculateDistance(candidatePos, targetPos);

        if (i > 0 && currentData !== null) {
            // heading check
            const prevPos = path[i - 1]!;
            const candidateHeading = calculateHeading(prevPos, candidatePos);
            const headingDiff = calculateHeadingDiff(
                currentData.heading,
                candidateHeading
            );
            if (headingDiff > 100) {
                console.debug(
                    LOG_PREFIX,
                    "skipping due to heading diff:",
                    headingDiff
                );
                continue;
            }
        }

        if (distanceToCur < closestPanoInFirstPathDistance) {
            closestPanoInFirstPathIndex = i;
            closestPanoInFirstPathDistance = distanceToCur;
        }
    }

    return [closestPanoInFirstPathIndex, closestPanoInFirstPathDistance];
}
