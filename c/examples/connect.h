/* Shared by the examples: open the device named on the command line.
 *   --host H [--port P]   TCP
 *   --usb [SERIAL]        USB (first device, or the one with that serial)
 *   (nothing)             UDP discovery, the first device that answers */
#ifndef EXAMPLE_CONNECT_H
#define EXAMPLE_CONNECT_H

#include "nebulosa.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const char *arg_value(int argc, char **argv, const char *name, const char *def)
{
    for (int i = 1; i < argc - 1; i++)
        if (strcmp(argv[i], name) == 0)
            return argv[i + 1];
    return def;
}

static int arg_flag(int argc, char **argv, const char *name)
{
    for (int i = 1; i < argc; i++)
        if (strcmp(argv[i], name) == 0)
            return i;
    return 0;
}

static nebulosa_t *connect_from_args(int argc, char **argv)
{
    nebulosa_t *d = NULL;
    int e, u = arg_flag(argc, argv, "--usb");
    const char *host = arg_value(argc, argv, "--host", NULL);
    uint16_t port = (uint16_t)atoi(arg_value(argc, argv, "--port", "0"));
    if (u)
    {
        const char *serial = (u + 1 < argc && argv[u + 1][0] != '-') ? argv[u + 1] : NULL;
        e = nebulosa_open_usb(&d, serial, 0);
    }
    else if (host)
        e = nebulosa_open_tcp(&d, host, port, 3000);
    else
    {
        nebulosa_net_device_t found[8];
        int n = nebulosa_discover(found, 8, 800);
        if (n <= 0)
        {
            fprintf(stderr, "no device found by discovery; use --host H or --usb\n");
            exit(1);
        }
        fprintf(stderr, "found %s\n", found[0].host);
        e = nebulosa_open_tcp(&d, found[0].host, port, 3000);
    }
    if (e)
    {
        fprintf(stderr, "connect: %s\n", nebulosa_strerror(e));
        exit(1);
    }
    return d;
}

#define TRY(x) do { int _e = (x); if (_e) { fprintf(stderr, "%s: %s\n", #x, nebulosa_strerror(_e)); exit(1); } } while (0)

#endif
