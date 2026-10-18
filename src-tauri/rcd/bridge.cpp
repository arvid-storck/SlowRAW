// SlowRAW adapter. RCD and border interpolation come unchanged from librtprocess.
#include "librtprocess.h"
#include <vector>
#include <new>

extern "C" int slowraw_rcd(int width, int height, const float *input,
                           float *output, const unsigned *pattern) noexcept {
    try {
        const size_t count = size_t(width) * size_t(height);
        std::vector<float> planes(3 * count);
        std::vector<const float *> raw(height);
        std::vector<float *> red(height), green(height), blue(height);
        for (int y = 0; y < height; ++y) {
            raw[y] = input + size_t(y) * width;
            red[y] = planes.data() + size_t(y) * width;
            green[y] = red[y] + count;
            blue[y] = green[y] + count;
        }
        const unsigned cfa[2][2] = {{pattern[0], pattern[1]}, {pattern[2], pattern[3]}};
        const int error = rcd_demosaic(width, height, raw.data(), red.data(), green.data(),
            blue.data(), cfa, [](double) { return false; }, 2, false, false);
        if (error) return error;
        for (size_t i = 0; i < count; ++i) {
            output[3*i] = planes[i];
            output[3*i+1] = planes[count+i];
            output[3*i+2] = planes[2*count+i];
        }
        return 0;
    } catch (...) {
        return -1;
    }
}
