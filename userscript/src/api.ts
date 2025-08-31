import { onProgress } from ".";
import { LOG_PREFIX } from "./constants";
import {
    completePathSegments,
    convertDestinationToPathId,
    destinationToPathIdMap,
    pathIdsToPathSegmentMetadata,
    pathIdToDestination,
    pathSegmentCosts,
} from "./map/lines";
import { getLat, getLng } from "./pos";
import { SETTINGS } from "./settings";

const BASE_API = "https://ir.matdoes.dev";

export interface PathfinderMessage {
    id: number;

    percent_done: number;
    estimated_seconds_remaining: number;
    best_path_cost: number;
    nodes_considered: number;
    elapsed_seconds: number;

    best_path_keep_prefix_length: number;
    best_path_append: GeoJSON.Position[];

    current_path_keep_prefix_length: number;
    current_path_append: GeoJSON.Position[];
}

let pfWs: WebSocket;
let queuedWebSocketMessages: string[] = [];

export let calculatingPathId: number | undefined = undefined;
let nextPathId = 0;

/**
 *
 * @param heading in degrees
 * @param start lng, lat
 * @param end lng, lat
 * @param startPano The ID of the current pano. If not passed, the start coords will get
 * snapped to the nearest pano instead.
 */
export function requestNewPath(
    heading: number,
    start: GeoJSON.Position,
    end: GeoJSON.Position,
    startPano?: string
) {
    const pathMetadata = {
        source: {
            pos: start,
            heading,
        },
        destination: end,
    };
    let alreadyKnownPathNodes = undefined;
    let alreadyKnownPathCost = undefined;

    const previousPathIdToSameDest = convertDestinationToPathId(end);
    if (previousPathIdToSameDest !== undefined) {
        console.debug(
            LOG_PREFIX,
            "we previously calculated a path to",
            end,
            "that we might be able to reuse"
        );

        // save the data in a variable for a bit just in case
        const oldPathMetadata = pathIdsToPathSegmentMetadata.get(
            previousPathIdToSameDest
        );
        const oldPathCost = pathSegmentCosts.get(previousPathIdToSameDest);
        const oldPathNodes = completePathSegments.get(previousPathIdToSameDest);

        // we're calculating a new path to the same destination, so forget everything we have
        // stored about the old path to avoid a memory leak
        pathIdsToPathSegmentMetadata.delete(previousPathIdToSameDest);
        pathSegmentCosts.delete(previousPathIdToSameDest);
        completePathSegments.delete(previousPathIdToSameDest);
        pathIdToDestination.delete(previousPathIdToSameDest);

        // we might be able to copy that old path (to avoid recalculating) if it had the same source too
        if (
            oldPathNodes !== undefined &&
            JSON.stringify(oldPathMetadata) === JSON.stringify(pathMetadata)
        ) {
            alreadyKnownPathNodes = oldPathNodes;
            alreadyKnownPathCost = oldPathCost;
            console.debug(
                LOG_PREFIX,
                "reusing old path",
                previousPathIdToSameDest,
                "to",
                end
            );
        }
    }

    console.debug(
        LOG_PREFIX,
        "destinationToPathIdMap",
        destinationToPathIdMap,
        end
    );

    // request a new path
    const pathId = nextPathId;
    calculatingPathId = pathId;
    nextPathId++;
    pathIdsToPathSegmentMetadata.set(pathId, pathMetadata);
    destinationToPathIdMap.set(`${getLat(end)},${getLng(end)}`, pathId);
    pathIdToDestination.set(pathId, end);

    if (alreadyKnownPathNodes !== undefined) {
        onProgress({
            id: calculatingPathId,
            percent_done: 1,
            estimated_seconds_remaining: 0,
            best_path_cost: alreadyKnownPathCost!,
            // these aren't used by the userscript so setting them to -1 is fine
            nodes_considered: -1,
            elapsed_seconds: -1,

            best_path_keep_prefix_length: 0,
            best_path_append: alreadyKnownPathNodes,
            current_path_keep_prefix_length: 0,
            current_path_append: [],
        });
        return;
    }

    sendWebSocketMessage({
        kind: "path",
        start: [getLat(start), getLng(start)],
        end: [getLat(end), getLng(end)],
        heading,
        start_pano: startPano,
        id: pathId,
        no_long_jumps: !SETTINGS.allow_long_jumps,
    });
}

/**
 * Send a message to the pathfinder's WebSocket, queuing it for later if the WebSocket is currently
 * closed.
 *
 * @param msg The object that will get converted into JSON and sent to
 * the server.
 */
export function sendWebSocketMessage(msg: Record<string, unknown>) {
    console.debug(LOG_PREFIX, "sending", msg, "to pathfinder websocket");
    if (pfWs.readyState !== 1) {
        console.debug(
            LOG_PREFIX,
            "websocket is closed, adding message to queue"
        );
        queuedWebSocketMessages.push(JSON.stringify(msg));
    } else {
        pfWs.send(JSON.stringify(msg));
    }
}

async function waitAndReconnect() {
    console.debug(LOG_PREFIX, "reconnecting to WebSocket");
    // this timeout is 10 seconds because of a firefox quirk that makes it delay creating websockets if you do it too fast
    await new Promise((r) => setTimeout(r, 10000));
    console.debug(LOG_PREFIX, "reconnecting...");
    connect();
}
function connect() {
    console.debug(LOG_PREFIX, "connecting to websocket");
    pfWs = new WebSocket(BASE_API.replace("http", "ws") + "/path");
    console.debug(LOG_PREFIX, "websocket created:", pfWs);

    pfWs.addEventListener("close", async () => {
        console.debug(LOG_PREFIX, "Pathfinder WebSocket closed.");
        waitAndReconnect();
    });
    pfWs.addEventListener("error", (e) => {
        console.error(LOG_PREFIX, "Pathfinder WebSocket error:", e);
        pfWs.close();
    });
    pfWs.addEventListener("open", () => {
        console.debug(LOG_PREFIX, "Pathfinder WebSocket connected.");

        for (const msg of queuedWebSocketMessages) {
            pfWs.send(msg);
        }
        queuedWebSocketMessages = [];
    });

    pfWs.addEventListener("message", (e) => {
        const data = JSON.parse(e.data);
        if (data.type === "progress") {
            onProgress(data);
        } else if (data.type === "error") {
            alert(data.message);
        }
    });
}
connect();

export async function waitUntilConnected() {
    if (pfWs.readyState !== 1) {
        await new Promise((res) => pfWs.addEventListener("open", res));
    }
}

export async function clearCalculatingPathId() {
    calculatingPathId = undefined;
}
