import { getOrderedStops, reorderStop, removeStop } from "./stops";
import { getLat, getLng } from "./pos";
import { SETTINGS } from "./settings";

let stopsMenuEl: HTMLDivElement | null = null;
let draggedStopIndex: number | null = null;

export function initStopsMenu(containerEl: HTMLElement) {
    stopsMenuEl = document.createElement("div");
    stopsMenuEl.className = "pathfinder-stops-menu";
    containerEl.appendChild(stopsMenuEl);
    rerenderStopsMenu();
}

export function rerenderStopsMenu() {
    if (!stopsMenuEl) return;
    
    const stops = getOrderedStops();
    stopsMenuEl.innerHTML = "";
    
    if (stops.length === 0 || !SETTINGS.show_stops_menu) {
        stopsMenuEl.style.display = "none";
        return;
    }
    
    stopsMenuEl.style.display = "block";
    
    const headerEl = document.createElement("div");
    headerEl.className = "pathfinder-stops-menu-header";
    headerEl.textContent = "Stops";
    stopsMenuEl.appendChild(headerEl);
    
    const listEl = document.createElement("div");
    listEl.className = "pathfinder-stops-list";
    stopsMenuEl.appendChild(listEl);
    
    stops.forEach((stop, index) => {
        const itemEl = document.createElement("div");
        itemEl.className = "pathfinder-stop-item";
        itemEl.draggable = true;
        itemEl.dataset.index = String(index);
        
        const numberEl = document.createElement("span");
        numberEl.className = "pathfinder-stop-item-number";
        numberEl.textContent = String(index + 1);
        itemEl.appendChild(numberEl);
                
        const controlsEl = document.createElement("div");
        controlsEl.className = "pathfinder-stop-item-controls";
        
        if (index > 0) {
            const upBtn = document.createElement("button");
            upBtn.className = "pathfinder-stop-btn pathfinder-stop-btn-up";
            upBtn.style.gridColumn = "1";
            upBtn.textContent = "↑";
            upBtn.title = "Move up";
            upBtn.onclick = (e) => {
                e.stopPropagation();
                reorderStop(index, index - 1);
            };
            controlsEl.appendChild(upBtn);
        }
        
        if (index < stops.length - 1) {
            const downBtn = document.createElement("button");
            downBtn.className = "pathfinder-stop-btn pathfinder-stop-btn-down";
            downBtn.style.gridColumn = "2";
            downBtn.textContent = "↓";
            downBtn.title = "Move down";
            downBtn.onclick = (e) => {
                e.stopPropagation();
                reorderStop(index, index + 1);
            };
            controlsEl.appendChild(downBtn);
        }
        
        const removeBtn = document.createElement("button");
        removeBtn.className = "pathfinder-stop-btn pathfinder-stop-btn-remove";
        removeBtn.style.gridColumn = "3";
        removeBtn.textContent = "×";
        removeBtn.title = "Remove stop";
        removeBtn.onclick = (e) => {
            e.stopPropagation();
            removeStop(stop);
        };
        controlsEl.appendChild(removeBtn);
        
        itemEl.appendChild(controlsEl);
        
        itemEl.addEventListener("dragstart", (e) => {
            draggedStopIndex = index;
            itemEl.classList.add("dragging");
            e.dataTransfer!.effectAllowed = "move";
        });
        
        itemEl.addEventListener("dragend", () => {
            itemEl.classList.remove("dragging");
            draggedStopIndex = null;
        });
        
        itemEl.addEventListener("dragover", (e) => {
            e.preventDefault();
            e.dataTransfer!.dropEffect = "move";
            itemEl.classList.add("drag-over");
        });
        
        itemEl.addEventListener("dragleave", () => {
            itemEl.classList.remove("drag-over");
        });
        
        itemEl.addEventListener("drop", (e) => {
            e.preventDefault();
            itemEl.classList.remove("drag-over");
            
            if (draggedStopIndex !== null && draggedStopIndex !== index) {
                reorderStop(draggedStopIndex, index);
            }
        });
        
        listEl.appendChild(itemEl);
    });
}
