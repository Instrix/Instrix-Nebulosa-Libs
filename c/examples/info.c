/* Print the device's info, stats and board status. */
#include "connect.h"

int main(int argc, char **argv)
{
    nebulosa_t *d = connect_from_args(argc, argv);
    nbp_info_t i;
    nbp_stats_t s;
    TRY(nebulosa_get_info(d, &i));
    TRY(nebulosa_get_stats(d, &s));
    printf("%s\n", nebulosa_description(d));
    printf("firmware   %u.%u.%u (%s)\n", nebulosa_version_major(i.fw_version), nebulosa_version_minor(i.fw_version),
           nebulosa_version_patch(i.fw_version), i.build);
    printf("fpga id    0x%08X\n", i.fpga_id);
    printf("base rate  %u S/s, link %u, caps 0x%X\n", i.base_rate_hz, i.link, i.caps);
    printf("uptime     %.1f s\n", s.uptime_ms / 1000.0);
    if (i.caps & NBP_CAP_BOARD)
    {
        nbp_board_status_t b;
        TRY(nebulosa_board_status(d, &b));
        printf("supplies   jack %.2f V, vbus %.2f V, main %.2f V\n", b.v_bj_mv / 1e3, b.v_vbus_mv / 1e3, b.v_main_mv / 1e3);
        printf("usb pd     state %u, %u mV %u mA\n", b.pd_state, b.pd_mv, b.pd_ma);
        printf("flags      0x%08X\n", b.flags);
    }
    nebulosa_close(d);
    return 0;
}
