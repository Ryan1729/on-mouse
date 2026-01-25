# On-Mouse

A command-line program that detects mouse movement and calls provided executables when the mouse changes from active to inactive, and vice versa, with a configurable time threshold.

```
$ on-mouse --help
OPTIONS:
    --on-active <on_active>
      Exectuable to run when mouse is detected to be actively moved

    --on-inactive <on_inactive>
      Exectuable to run when mouse is detected to be not actively moved

    -q, --quiet
      Whether to supress output of the current state

    --min-movement-gap <min_movement_gap>
      The minimum gap between two readings to consider the mouse inactive, in milliseconds.
      Defaults to one second.

    -h, --help
      Prints help information.
```
