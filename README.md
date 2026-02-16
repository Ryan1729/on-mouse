# On-Mouse

A command-line program that detects mouse movement and calls provided executables when the mouse changes from active to inactive, and vice versa, with a configurable time threshold.

```
$ on-mouse --help
on-mouse

OPTIONS:
    --on-active <path>
      Exectuable to run when mouse is detected to be actively moved

    --on-inactive <path>
      Exectuable to run when mouse is detected to be not actively moved

    -q, --quiet
      Whether to supress output of the current state

    --chart
      Whether to display a chart instead of default, basic print output of the current state

    --min-movement-gap <milliseconds>
      The minimum gap between two readings to consider the mouse inactive, in milliseconds.
      Defaults to one second.

    --grab-device <device_name>
      The name of a device to grap and thus block any other applications from seeing.
      The passed name indicates which device to grab. If passed, any other mice will be
      ignored by this program.
      On Linux, the name for a given device can be found using the `evdev` application.
      Currently not supported on Windows.

    --version
      Output the version and exit

    -h, --help
      Prints help information.

```
