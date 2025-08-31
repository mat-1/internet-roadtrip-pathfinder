// @ts-check

import resolvePlugin from "@rollup/plugin-node-resolve";
import typescriptPlugin from "@rollup/plugin-typescript";
import userscriptPlugin from "rollup-plugin-userscript";
import pkg from "./package.json" with { type: "json" };

/** @type {import("rollup").RollupOptions} */
export default {
    input: "src/index.ts",
    output: {
        file: "../static/pathfinder.user.js",
        format: "iife",
        name: "Pathfinder"
    },
    plugins: [
        typescriptPlugin(),
        resolvePlugin(),
        userscriptPlugin((meta) =>
            meta
                .replace("PACKAGE_JSON_VERSION", pkg.version)
                .replace("PACKAGE_JSON_AUTHOR", pkg.author)
                .replace("PACKAGE_JSON_DESCRIPTION", pkg.description)
                .replace("PACKAGE_JSON_LICENSE", pkg.license)
        ),
    ],
    onLog(level, log, handler) {
        if (log.code === 'CIRCULAR_DEPENDENCY') {
            // Ignore circular dependency warnings
            return;
        }
        handler(level, log);
    }
};
