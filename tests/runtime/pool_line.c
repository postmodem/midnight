/*
 * The thread pool with a main of its own: writes the pool's out-of-memory
 * line, as JSON when the argument is `json`, and aborts as the pool does.
 */

#include "../../runtime/midnight_pool.c"

int main(int argc, char **argv) {
    MnPoolOptions options = {0};
    options.threads = 1;
    options.json = argc > 1 && strcmp(argv[1], "json") == 0;
    mn_pool_start(&options);
    pool_out_of_memory();
}
