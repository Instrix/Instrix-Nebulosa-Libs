/* Mean / min / max current, mean voltage, charge and energy over T seconds.
 *   nebulosa_average_current [--host H | --usb] [--seconds 10] [--log2n 0] */
#include "connect.h"

int main(int argc, char **argv)
{
    double secs = atof(arg_value(argc, argv, "--seconds", "10"));
    int log2n = atoi(arg_value(argc, argv, "--log2n", "0"));
    nebulosa_t *d = connect_from_args(argc, argv);
    uint32_t rate;
    TRY(nebulosa_set_decimation(d, (uint8_t)log2n, NBP_DECIM_AVERAGE, &rate));
    TRY(nebulosa_set_range_auto(d, 0, 5));
    TRY(nebulosa_start_stream(d, NBP_START_FLUSH));

    static nebulosa_sample_t buf[16384];
    double si = 0, sv = 0, sp = 0, imin = 1e9, imax = -1e9, t_first = -1, t_last = 0;
    uint64_t n_all = 0;
    while (t_first < 0 || t_last - t_first < secs)
    {
        int n = nebulosa_read_samples(d, buf, 16384, 1000);
        if (n < 0)
        {
            fprintf(stderr, "read: %s\n", nebulosa_strerror(n));
            break;
        }
        for (int i = 0; i < n; i++)
        {
            const nebulosa_sample_t *s = &buf[i];
            si += s->current;
            sv += s->voltage;
            sp += s->current * s->voltage;
            if (s->current < imin) imin = s->current;
            if (s->current > imax) imax = s->current;
            t_last = nebulosa_sample_time(s);
            if (t_first < 0)
                t_first = t_last;
        }
        n_all += (uint64_t)n;
    }
    nebulosa_stop_stream(d);
    double T = t_last - t_first;
    printf("%llu samples over %.3f s (%u S/s)\n", (unsigned long long)n_all, T, rate);
    printf("current  mean %.6g A  min %.6g A  max %.6g A\n", si / n_all, imin, imax);
    printf("voltage  mean %.4f V\n", sv / n_all);
    printf("charge   %.6g C (%.6g mAh)\n", si / n_all * T, si / n_all * T / 3.6);
    printf("energy   %.6g J\n", sp / n_all * T);
    nebulosa_close(d);
    return 0;
}
