export const sleep = (ms: number) => new Promise((res) => setTimeout(res, ms));

export function prettyTime(seconds: number): string {
    if (seconds < 0) return "now";

    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    const secondsLeft = Math.floor(seconds % 60);
    const msLeft = Math.floor((seconds * 1000) % 1000);
    if (hours > 0) {
        return `${hours}h ${minutes}m ${secondsLeft}s`;
    } else if (minutes > 0) {
        return `${minutes}m ${secondsLeft}s`;
    } else if (secondsLeft > 0) {
        return `${secondsLeft}s`;
    } else {
        return `${msLeft}ms`;
    }
}
