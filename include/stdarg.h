#pragma once

// FIXME: I don't know what i copy pasted here
typedef char *va_list;
#define va_start(ap,parmn) (void)((ap) = (char*)(&(parmn) + 1))
#define va_end(ap) (void)((ap) = 0)
#define va_arg(ap, type) \
    (*(type*)va_arg_ptr(ap, sizeof(type)))
#define va_arg_ptr(ap, size) \
    ( ((ap) = ((ap) + size)) - size)
