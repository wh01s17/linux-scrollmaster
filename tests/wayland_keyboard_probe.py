"""Optional real compositor probe: creates keyboard with zero modifiers; no HID writes."""
from pathlib import Path
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'bridge'))
from modifiers import WaylandKeyboard

if __name__ == '__main__':
    keyboard = WaylandKeyboard()
    try:
        print('PASS Wayland virtual keyboard initialized; no modifiers pressed', flush=True)
    finally:
        keyboard.close()
