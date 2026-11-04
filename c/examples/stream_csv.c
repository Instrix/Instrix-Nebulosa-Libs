/* Stream for N seconds and write time, current, voltage, range as CSV.
 *   nebulosa_stream_csv [--host H | --usb] [--seconds 5] [--log2n 4] [--out file.csv] */
#include "connect.h"

int main(int argc, char **argv)
{
    double secs = atof(arg_value(argc, argv, "--seconds", "5"));
    int log2n = atoi(arg_value(argc, argv, "--log2n", "4"));
    const char *path = arg_value(argc, argv, "--out", NULL);
    FILE *f = path ? fopen(path, "w") : stdout;
    if (!f)
    {
        perror(path);
        return 1;
    }
    nebulosa_t *d = connect_from_args(argc, argv);
    uint32_t rate;
    TRY(nebulosa_set_decimation(d, (uint8_t)log2n, NBP_DECIM_AVERAGE, &rate));
    fprintf(stderr, "%u samples/s for %.1f s\n", rate, secs);
    TRY(nebulosa_start_stream(d, NBP_START_FLUSH));

    static nebulosa_sample_t buf[8192];
    uint64_t total = 0, want = (uint64_t)(secs * rate);
    double t0 = -1;
    fprintf(f, "time_s,current_a,voltage_v,range,dirty\n");
    while (total < want)
    {
        int n = nebulosa_read_samples(d, buf, 8192, 1000);
        if (n < 0)
        {
            fprintf(stderr, "read: %s\n", nebulosa_strerror(n));
            break;
        }
        for (int i = 0; i < n && total < want; i++, total++)
        {
            double t = nebulosa_sample_time(&buf[i]);
            if (t0 < 0)
                t0 = t;
            fprintf(f, "%.7f,%.9g,%.6f,%u,%u\n", t - t0, buf[i].current, buf[i].voltage, buf[i].range, buf[i].dirty);
        }
    }
    nebulosa_stop_stream(d);
    nebulosa_stream_counters_t c;
    nebulosa_stream_counters(d, &c);
    fprintf(stderr, "%llu samples, %llu blocks lost\n", (unsigned long long)total, (unsigned long long)c.lost_blocks);
    nebulosa_close(d);
    if (path)
        fclose(f);
    return 0;
}
