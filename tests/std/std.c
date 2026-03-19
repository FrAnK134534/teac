#include <stdarg.h>
#include <stdio.h>
#include <sys/time.h>

#define MAX_TIMERS 1024

static struct timeval timer_start_ts;
static struct timeval timer_end_ts;
static int timer_l1[MAX_TIMERS];
static int timer_l2[MAX_TIMERS];
static int timer_h[MAX_TIMERS];
static int timer_m[MAX_TIMERS];
static int timer_s[MAX_TIMERS];
static int timer_us[MAX_TIMERS];
static int timer_idx;

int getint(void) {
    int t;
    scanf("%d", &t);
    return t;
}

int getch(void) {
    char c;
    scanf("%c", &c);
    return (int)c;
}

void putint(int a) { printf("%d", a); }
void putch(int a) { printf("%c", a); }

float getfloat(void) {
    float t;
    scanf("%f", &t);
    return t;
}

void putfloat(float a) { printf("%f", a); }

void putarray(int n, int a[]) {
    printf("%d:", n);
    for (int i = 0; i < n; i++) printf(" %d", a[i]);
    printf("\n");
}

__attribute__((constructor)) static void timer_before_main(void) {
    for (int i = 0; i < MAX_TIMERS; i++)
        timer_h[i] = timer_m[i] = timer_s[i] = timer_us[i] = 0;
    timer_idx = 1;
}

__attribute__((destructor)) static void timer_after_main(void) {
    long long total_us = 0;

    for (int i = 1; i < timer_idx; i++) {
#ifdef BENCHMARK
        fprintf(stderr, "Timer@%04d-%04d: %dH-%dM-%dS-%dus\n", timer_l1[i],
                timer_l2[i], timer_h[i], timer_m[i], timer_s[i], timer_us[i]);
#endif
        total_us += (long long)timer_us[i] + (long long)timer_s[i] * 1000000LL +
                    (long long)timer_m[i] * 60LL * 1000000LL +
                    (long long)timer_h[i] * 60LL * 60LL * 1000000LL;
    }

    int th = (int)(total_us / (60LL * 60LL * 1000000LL));
    total_us %= 60LL * 60LL * 1000000LL;

    int tm = (int)(total_us / (60LL * 1000000LL));
    total_us %= 60LL * 1000000LL;

    int ts = (int)(total_us / 1000000LL);
    int tus = (int)(total_us % 1000000LL);
#ifdef BENCHMARK
    fprintf(stderr, "TOTAL: %dH-%dM-%dS-%dus\n", th, tm, ts, tus);
#endif
}

void timer_start(int lineno) {
    timer_l1[timer_idx] = lineno;
    gettimeofday(&timer_start_ts, NULL);
}

void timer_stop(int lineno) {
    gettimeofday(&timer_end_ts, NULL);
    timer_l2[timer_idx] = lineno;

    long long dur_us =
        (long long)(timer_end_ts.tv_sec - timer_start_ts.tv_sec) * 1000000LL +
        (long long)(timer_end_ts.tv_usec - timer_start_ts.tv_usec);

    if (dur_us < 0) dur_us = 0;

    timer_h[timer_idx] = (int)(dur_us / (60LL * 60LL * 1000000LL));
    dur_us %= 60LL * 60LL * 1000000LL;

    timer_m[timer_idx] = (int)(dur_us / (60LL * 1000000LL));
    dur_us %= 60LL * 1000000LL;

    timer_s[timer_idx] = (int)(dur_us / 1000000LL);
    timer_us[timer_idx] = (int)(dur_us % 1000000LL);

    timer_idx++;
    if (timer_idx >= MAX_TIMERS) timer_idx = MAX_TIMERS - 1;
}
