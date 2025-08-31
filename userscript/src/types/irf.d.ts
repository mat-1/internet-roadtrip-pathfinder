import InternetRoadtripFramework from "internet-roadtrip-framework";

declare global {
    const IRF = InternetRoadtripFramework;

    /**
     * A message from the Internet Roadtrip WebSocket
     */
    type RoadtripMessage = Parameters<
        Awaited<typeof IRF.vdom.container>["methods"]["updateData"]
    >[0];

    /**
     * Non-exclusive
     */
    type MinimapTricksContext =
        | "Side"
        | "Map"
        | "Car"
        | "Marker"
        | "Pathfinder"
        | "Pathfinder destination"
        | "Pathfinder stop";

    type MinimapTricksAddContextFn = (
        name: MinimapTricksContext,
        available: string[]
    ) => void;

    interface MinimapTricksButton {
        icon: string;
        name: string;
        callback: (control: MinimapTricksControl) => void;
        context?: MinimapTricksContext[];
        context_button: HTMLButtonElement;
        context_icon: HTMLSpanElement;
        context_checkbox: HTMLInputElement;
    }
    interface MinimapTricksSideButton extends MinimapTricksButton {
        side_button: HTMLButtonElement;
        side_icon: HTMLSpanElement;
        side_checkbox: HTMLInputElement;
    }

    interface MinimapTricksControl {
        lat: number;
        lng: number;
        marker: maplibregl.Marker | undefined;
        context: MinimapTricksContext;

        _m_cont: HTMLElement;

        addButton(
            icon: string,
            name: string,
            callback: (control: MinimapTricksControl) => void,
            context: Exclude<MinimapTricksContext, "Side">[]
        ): MinimapTricksButton;
        addButton(
            icon: string,
            name: string,
            callback: (control: MinimapTricksControl) => void,
            context: undefined | MinimapTricksContext[]
        ): MinimapTricksSideButton;

        openMenu(
            context: MinimapTricksContext,
            lat: number,
            lng: number,
            left: number,
            top: number,
            data: object
        ): void;
    }

    interface Window {
        _MMT_addContext: MinimapTricksAddContextFn;
        _MMT_control: MinimapTricksControl;
    }
}
