#ifndef ACELUS_PROCGUARD_H
#define ACELUS_PROCGUARD_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef uint64_t procguard_handle;

#define PROCGUARD_OK 0
#define PROCGUARD_ERR_UNSUPPORTED -1
#define PROCGUARD_ERR_SYSTEM -2
#define PROCGUARD_ERR_INVALID -3

int procguard_create(procguard_handle *out);
int procguard_prepare_child(void);
int procguard_adopt(procguard_handle *handle, uint64_t pid);
int procguard_terminate(procguard_handle handle);
int procguard_destroy(procguard_handle handle);

#ifdef __cplusplus
}
#endif

#endif
