cmake_minimum_required(VERSION 3.14)
project(nebulosa VERSION 0.1.0 LANGUAGES C)

# libnebulosa: NBP host library for the INSTRIX Nebulosa (TCP, UDP discovery,
# USB via libusb-1.0).
set(NEBULOSA_USB AUTO CACHE STRING "USB support through libusb-1.0: AUTO (when found), ON, OFF")
set_property(CACHE NEBULOSA_USB PROPERTY STRINGS AUTO ON OFF)
option(NEBULOSA_BUILD_EXAMPLES "Build the examples" ${PROJECT_IS_TOP_LEVEL})
option(NEBULOSA_BUILD_TESTS "Build the tests" ${PROJECT_IS_TOP_LEVEL})
option(BUILD_SHARED_LIBS "Build a shared library" OFF)

set(NEBULOSA_SPEC_DIR ${CMAKE_CURRENT_SOURCE_DIR}/../spec CACHE PATH "Directory holding nbp.h")

add_library(nebulosa src/nebulosa.c)
add_library(nebulosa::nebulosa ALIAS nebulosa)
target_include_directories(nebulosa PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<BUILD_INTERFACE:${NEBULOSA_SPEC_DIR}>
    $<INSTALL_INTERFACE:include>)
target_compile_features(nebulosa PRIVATE c_std_99)
target_compile_definitions(nebulosa PRIVATE NEBULOSA_BUILD)
if(BUILD_SHARED_LIBS)
    target_compile_definitions(nebulosa PUBLIC NEBULOSA_SHARED)
endif()
if(MSVC)
    target_compile_options(nebulosa PRIVATE /W4)
else()
    target_compile_options(nebulosa PRIVATE -Wall -Wextra -Wno-address-of-packed-member)
endif()
if(WIN32)
    target_link_libraries(nebulosa PRIVATE ws2_32)
endif()

if(NOT NEBULOSA_USB STREQUAL "OFF")
    find_package(PkgConfig QUIET)
    if(PkgConfig_FOUND)
        pkg_check_modules(LIBUSB IMPORTED_TARGET libusb-1.0)
    endif()
    if(LIBUSB_FOUND)
        target_link_libraries(nebulosa PRIVATE PkgConfig::LIBUSB)
        target_compile_definitions(nebulosa PRIVATE NEBULOSA_WITH_USB)
        message(STATUS "nebulosa: USB support (libusb-1.0 ${LIBUSB_VERSION})")
    elseif(NEBULOSA_USB STREQUAL "ON")
        message(FATAL_ERROR "nebulosa: NEBULOSA_USB=ON but libusb-1.0 not found")
    else()
        message(STATUS "nebulosa: libusb-1.0 not found, TCP only")
    endif()
endif()

include(GNUInstallDirs)
install(TARGETS nebulosa EXPORT nebulosaTargets
    ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
install(FILES include/nebulosa.h ${NEBULOSA_SPEC_DIR}/nbp.h DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
install(EXPORT nebulosaTargets NAMESPACE nebulosa:: DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/nebulosa)

if(NEBULOSA_BUILD_EXAMPLES)
    foreach(ex info stream_csv average_current board_leds)
        add_executable(nebulosa_${ex} examples/${ex}.c)
        target_link_libraries(nebulosa_${ex} PRIVATE nebulosa)
    endforeach()
endif()

if(NEBULOSA_BUILD_TESTS)
    enable_testing()
    add_executable(nebulosa_test tests/test.c)
    target_link_libraries(nebulosa_test PRIVATE nebulosa)
    find_package(Python3 COMPONENTS Interpreter)
    add_test(NAME unit COMMAND nebulosa_test)
    if(Python3_FOUND)
        add_test(NAME sim COMMAND ${Python3_EXECUTABLE} ${CMAKE_CURRENT_SOURCE_DIR}/tests/run_with_sim.py
            ${CMAKE_CURRENT_SOURCE_DIR}/../tools/nebulosa_sim.py $<TARGET_FILE:nebulosa_test>)
    endif()
endif()
