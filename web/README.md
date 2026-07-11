# Rustagon Web App

## Development

Install `deno` for your device. Then run:

    deno task dev

## Building

The firmware requires the web app to be crunched into a single GZIP'ed binary:

    deno task build
    deno task compress

When the firmware is next built, it will automatically source the binary from `bundle/index.html.gz`.
