export let map: maplibregl.Map;

export async function initMap() {
    map = await IRF.vdom.map.then((mapVDOM) => mapVDOM.state.map);
}
