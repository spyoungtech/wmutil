# wmutil

Utility functions for getting monitor (display) information on Windows. 


## Installation

Requires Python 3.8+ and Windows

```commandline
pip install wmutil
```

## Usage

```python
import wmutil


# Enumerate all monitors
print('Enumerating monitors:')
for monitor in wmutil.enumerate_monitors():
    # Print monitor attributes
    print(monitor, monitor.name, monitor.size, monitor.position, monitor.refresh_rate_millihertz, monitor.handle, sep='\n\t')


# Get primary monitor
primary_monitor = wmutil.get_primary_monitor()

# Get a monitor based on point coordinates
monitor = wmutil.get_monitor_from_point(0, 0)

# compare monitor objects
if monitor == primary_monitor:
   print('it is the primary monitor')


# Get monitor from an HWND
from ahk import AHK  # pip install ahk[binary]
ahk = AHK()

window = ahk.active_window
hwnd = int(window.id, 0)
monitor_for_active_window = wmutil.get_window_monitor(hwnd)
print(window.title, 'is using monitor', monitor_for_active_window.name)

```

Example output:

```
Enumerating monitors:
<wmutil.Monitor object; handle=491197379>
        \\.\DISPLAY1
        (1920, 1080)
        (-3840, -418)
        60000
        491197379
<wmutil.Monitor object; handle=85595795>
        \\.\DISPLAY2
        (3440, 1440)
        (0, 0)
        60000
        85595795
it is the primary monitor

Untitled - Notepad is using monitor \\.\DISPLAY2
```

**Changing the primary monitor:**

You can use the `set_primary_monitor` function, which accepts a display name or you can use the `set_primary` method of a `Monitor` object to change the 
primary monitor. If the monitor is already the primary monitor, no change will be made and the operation is considered successful. Returns `True` when successful and 
`False` when not successful. If an invalid monitor name is given, an exception is raised.

```python
import wmutil
monitor: wmutil.Monitor  # assume this is already defined

wmutil.set_primary_monitor(monitor.name)
# or
monitor.set_primary()
```



Notes:

- `monitor.size` may not necessarily reflect the monitor's resolution, but rather is the geometry used for drawing or moving windows
