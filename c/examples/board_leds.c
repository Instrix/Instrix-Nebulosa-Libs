/* Walk a light across the five board LEDs, then give them back. */
#include "connect.h"
#ifdef _WIN32
#include <windows.h>
#define sleep_ms(ms) Sleep(ms)
#else
#include <time.h>
static void sleep_ms(int ms) { struct timespec t = { ms / 1000, (ms % 1000) * 1000000L }; nanosleep(&t, NULL); }
#endif

int main(int argc, char **argv)
{
    nebulosa_t *d = connect_from_args(argc, argv);
    for (int round = 0; round < 3; round++)
        for (int i = 0; i < 5; i++)
        {
            TRY(nebulosa_set_leds(d, (uint8_t)(1u << i)));
            sleep_ms(120);
        }
    TRY(nebulosa_leds_auto(d));
    nebulosa_close(d);
    return 0;
}
