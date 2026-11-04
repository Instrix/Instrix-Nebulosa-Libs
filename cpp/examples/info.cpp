// Print device info and board status.
#include "connect.hpp"
#include <cstdio>
#include <iostream>

int main(int argc, char **argv) try {
    auto dev = connect(argc, argv);
    auto i = dev.info();
    std::cout << dev.description() << "\n"
              << "firmware  " << nebulosa::version_string(i.fw_version) << " (" << nebulosa::build_string(i) << ")\n"
              << "base rate " << i.base_rate_hz << " S/s, caps 0x" << std::hex << i.caps << std::dec << "\n";
    if (i.caps & NBP_CAP_BOARD) {
        auto b = dev.board_status();
        std::printf("supplies  jack %.2f V, vbus %.2f V, main %.2f V\n", b.v_bj_mv / 1e3, b.v_vbus_mv / 1e3, b.v_main_mv / 1e3);
    }
} catch (const nebulosa::Error &e) {
    std::cerr << e.what() << "\n";
    return 1;
}
