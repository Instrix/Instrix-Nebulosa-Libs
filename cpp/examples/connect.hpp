// Shared by the examples: --host H [--port P] | --usb [SERIAL] | (discovery)
#pragma once
#include "nebulosa.hpp"
#include <cstdlib>
#include <string>

inline std::string arg(int argc, char **argv, const std::string &name, const std::string &def = "") {
    for (int i = 1; i < argc - 1; i++)
        if (name == argv[i])
            return argv[i + 1];
    return def;
}

inline nebulosa::Device connect(int argc, char **argv) {
    for (int i = 1; i < argc; i++)
        if (std::string(argv[i]) == "--usb") {
            if (i + 1 < argc && argv[i + 1][0] != '-')
                return nebulosa::Device::usb(std::string(argv[i + 1]));
            return nebulosa::Device::usb();
        }
    std::string host = arg(argc, argv, "--host");
    if (!host.empty())
        return nebulosa::Device::tcp(host, static_cast<uint16_t>(std::stoi(arg(argc, argv, "--port", "20034"))));
    return nebulosa::Device::first_on_network();
}
