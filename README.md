# probe-rs-vscode README

Debugger plugin for vscode, based on (probe-rs)[https://github.com/probe-rs/probe-rs].

Currently in *early alpha stage*, except for halting and running not a lot is working yet.


## Development setup

The best way to debug and develop the plugin is to start the debug-adapter in
server mode, and then attach to the server from vscode. The server can be
started with the following command:

```bash
cargo run -- --server --port 8800
```


To run the vscode extension, a new windows of vscode containing the extension
can be launched using the `.vscode/launch.json` configuration. Pressing `F5`
should open a new window which contains the extension. In that new window,
open a project you want to debug, and then launch the extension using a configuration similiar to this:

```json
{
    // Use IntelliSense to learn about possible attributes.
    // Hover to view descriptions of existing attributes.
    // For more information, visit: https://go.microsoft.com/fwlink/?linkid=830387
    "version": "0.2.0",
        {
            "type": "probe_rs",
            "request": "attach",
            "name": "Example: gpio_hal_blinky, attach to debugger",
            "program": "${workspaceRoot}/target/thumbv6m-none-eabi/debug/examples/gpio_hal_blinky",
            "cwd" "${workspaceRoot}",
            "reset": true,
            "halt_after_reset": false,
            "server_mode": true,
            "server_port": 8800,
            "chip": "nrf5182"
        }
    ]
}
```

