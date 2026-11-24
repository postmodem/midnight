#include "gremlin_rt.h"

#include <stdio.h>
#include <stdlib.h>

GrWorld gr_world;
GrTime gr_time = {0, 1.0 / 60, 0.0};

void gr_start(const GrProgram *program) { gr_world.program = program; }

void gr_finish(void) { fflush(stdout); }

void gr_out_bytes(const char *bytes, int64_t len) {
    fwrite(bytes, 1, (size_t)len, stdout);
}

void gr_out_str(GrString s) {
    if (s) gr_out_bytes(s->bytes, s->len);
}

void gr_out_newline(void) { fputc('\n', stdout); }
