
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>

/*
 * Native C Rust Lexer & Delimiter/Span Scanner for tree_sitter_rust
 * Compiled with GCC -O3 -shared -fPIC
 */

typedef struct {
    uint32_t kind;       /* 1=ident, 2=punct, 3=op, 4=literal, 5=lifetime, 6=comment, 7=attribute */
    uint32_t start_byte;
    uint32_t end_byte;
    uint32_t start_row;
    uint32_t start_col;
    uint32_t end_row;
    uint32_t end_col;
    int32_t  error_code; /* 0=ok, 1=unclosed_delim, 2=unmatched_close, 3=mismatched_close */
} CToken;

static inline bool is_ident_start(uint8_t c) {
    return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' || c >= 128;
}

static inline bool is_ident_cont(uint8_t c) {
    return is_ident_start(c) || (c >= '0' && c <= '9');
}

int32_t ts_rust_tokenize(
    const uint8_t *src,
    uint32_t len,
    CToken *out_tokens,
    uint32_t max_tokens,
    int32_t *out_delim_err_line,
    int32_t *out_delim_err_col,
    char *out_delim_err_msg,
    uint32_t max_err_msg
) {
    uint32_t i = 0;
    uint32_t row = 0;
    uint32_t line_start = 0;
    uint32_t count = 0;

    /* Delimiter stack for fast C-level syntax validation */
    char stack_ch[4096];
    uint32_t stack_row[4096];
    uint32_t stack_col[4096];
    int sp = 0;

    *out_delim_err_line = 0;
    *out_delim_err_col = 0;
    if (max_err_msg > 0) out_delim_err_msg[0] = '\0';

    while (i < len && count < max_tokens) {
        uint8_t ch = src[i];

        if (ch == '\n') {
            i++;
            row++;
            line_start = i;
            continue;
        }
        if (ch == ' ' || ch == '\t' || ch == '\r') {
            i++;
            continue;
        }

        uint32_t s_byte = i;
        uint32_t s_row = row;
        uint32_t s_col = i - line_start;

        /* Line comment */
        if (ch == '/' && i + 1 < len && src[i + 1] == '/') {
            while (i < len && src[i] != '\n') i++;
            out_tokens[count++] = (CToken){6, s_byte, i, s_row, s_col, row, i - line_start, 0};
            continue;
        }

        /* Nested Block comment */
        if (ch == '/' && i + 1 < len && src[i + 1] == '*') {
            int depth = 1;
            i += 2;
            while (i < len && depth > 0) {
                if (src[i] == '/' && i + 1 < len && src[i + 1] == '*') {
                    depth++;
                    i += 2;
                } else if (src[i] == '*' && i + 1 < len && src[i + 1] == '/') {
                    depth--;
                    i += 2;
                } else {
                    if (src[i] == '\n') {
                        row++;
                        line_start = i + 1;
                    }
                    i++;
                }
            }
            out_tokens[count++] = (CToken){6, s_byte, i, s_row, s_col, row, i - line_start, 0};
            continue;
        }

        /* Raw string r#"..."# or br#"..."# */
        if ((ch == 'r' || (ch == 'b' && i + 1 < len && src[i + 1] == 'r')) ) {
            uint32_t scan = (ch == 'b') ? i + 2 : i + 1;
            uint32_t hashes = 0;
            while (scan < len && src[scan] == '#') {
                hashes++;
                scan++;
            }
            if (scan < len && src[scan] == '"') {
                i = scan + 1;
                while (i < len) {
                    if (src[i] == '\n') {
                        row++;
                        line_start = i + 1;
                    }
                    if (src[i] == '"') {
                        uint32_t h = 0;
                        while (i + 1 + h < len && h < hashes && src[i + 1 + h] == '#') h++;
                        if (h == hashes) {
                            i += 1 + hashes;
                            break;
                        }
                    }
                    i++;
                }
                out_tokens[count++] = (CToken){4, s_byte, i, s_row, s_col, row, i - line_start, 0};
                continue;
            }
        }

        /* String literal */
        if (ch == '"' || (ch == 'b' && i + 1 < len && src[i + 1] == '"')) {
            if (ch == 'b') i++;
            i++;
            while (i < len) {
                if (src[i] == '\\') {
                    if (i + 1 < len && src[i + 1] == '\n') {
                        row++;
                        line_start = i + 2;
                    }
                    i += 2;
                    continue;
                }
                if (src[i] == '\n') {
                    row++;
                    line_start = i + 1;
                }
                if (src[i] == '"') {
                    i++;
                    break;
                }
                i++;
            }
            out_tokens[count++] = (CToken){4, s_byte, i, s_row, s_col, row, i - line_start, 0};
            continue;
        }

        /* Lifetime or char literal */
        if (ch == '\'') {
            if (i + 2 < len && src[i + 1] != '\\' && src[i + 2] == '\'') {
                i += 3;
                out_tokens[count++] = (CToken){4, s_byte, i, s_row, s_col, row, i - line_start, 0};
                continue;
            }
            if (i + 1 < len && src[i + 1] == '\\') {
                i += 2;
                while (i < len && src[i] != '\'' && src[i] != '\n') i++;
                if (i < len && src[i] == '\'') i++;
                out_tokens[count++] = (CToken){4, s_byte, i, s_row, s_col, row, i - line_start, 0};
                continue;
            }
            if (i + 1 < len && is_ident_start(src[i + 1])) {
                i += 2;
                while (i < len && is_ident_cont(src[i])) i++;
                out_tokens[count++] = (CToken){5, s_byte, i, s_row, s_col, row, i - line_start, 0};
                continue;
            }
            i++;
            out_tokens[count++] = (CToken){2, s_byte, i, s_row, s_col, row, i - line_start, 0};
            continue;
        }

        /* Attribute #[...] or #![...] */
        if (ch == '#' && i + 1 < len && (src[i + 1] == '[' || (src[i + 1] == '!' && i + 2 < len && src[i + 2] == '['))) {
            i += (src[i + 1] == '!') ? 3 : 2;
            int depth = 1;
            while (i < len && depth > 0) {
                if (src[i] == '"') {
                    i++;
                    while (i < len && src[i] != '"') {
                        if (src[i] == '\\') i++;
                        if (i < len && src[i] == '\n') { row++; line_start = i + 1; }
                        i++;
                    }
                    if (i < len) i++;
                    continue;
                }
                if (src[i] == '[') depth++;
                else if (src[i] == ']') depth--;
                if (src[i] == '\n') { row++; line_start = i + 1; }
                i++;
            }
            out_tokens[count++] = (CToken){7, s_byte, i, s_row, s_col, row, i - line_start, 0};
            continue;
        }

        /* Identifier */
        if (is_ident_start(ch)) {
            i++;
            while (i < len && is_ident_cont(src[i])) i++;
            out_tokens[count++] = (CToken){1, s_byte, i, s_row, s_col, row, i - line_start, 0};
            continue;
        }

        /* Number literal */
        if (ch >= '0' && ch <= '9') {
            i++;
            while (i < len && (is_ident_cont(src[i]) || (src[i] == '.' && i + 1 < len && src[i + 1] >= '0' && src[i + 1] <= '9'))) {
                i++;
            }
            out_tokens[count++] = (CToken){4, s_byte, i, s_row, s_col, row, i - line_start, 0};
            continue;
        }

        /* Multi-character operators */
        if (i + 1 < len) {
            uint8_t n = src[i + 1];
            if ((ch == ':' && n == ':') || (ch == '-' && n == '>') || (ch == '=' && n == '>') ||
                (ch == '=' && n == '=') || (ch == '!' && n == '=') || (ch == '<' && n == '=') ||
                (ch == '>' && n == '=') || (ch == '&' && n == '&') || (ch == '|' && n == '|') ||
                (ch == '.' && n == '.') || (ch == '+' && n == '=') || (ch == '-' && n == '=') ||
                (ch == '*' && n == '=') || (ch == '/' && n == '=')) {
                i += 2;
                if (ch == '.' && n == '.' && i < len && src[i] == '=') i++;
                out_tokens[count++] = (CToken){3, s_byte, i, s_row, s_col, row, i - line_start, 0};
                continue;
            }
        }

        /* Delimiters & single-char punctuation */
        if (ch == '{' || ch == '(' || ch == '[') {
            if (sp < 4096) {
                stack_ch[sp] = (char)ch;
                stack_row[sp] = s_row + 1;
                stack_col[sp] = s_col + 1;
                sp++;
            }
            i++;
            out_tokens[count++] = (CToken){2, s_byte, i, s_row, s_col, row, i - line_start, 0};
            continue;
        }
        if (ch == '}' || ch == ')' || ch == ']') {
            char expected = (ch == '}') ? '{' : ((ch == ')') ? '(' : '[');
            int32_t err = 0;
            if (sp == 0) {
                err = 2;
                if (*out_delim_err_line == 0) {
                    *out_delim_err_line = (int32_t)(s_row + 1);
                    *out_delim_err_col = (int32_t)(s_col + 1);
                }
            } else {
                sp--;
                if (stack_ch[sp] != expected) {
                    err = 3;
                    if (*out_delim_err_line == 0) {
                        *out_delim_err_line = (int32_t)(s_row + 1);
                        *out_delim_err_col = (int32_t)(s_col + 1);
                    }
                }
            }
            i++;
            out_tokens[count++] = (CToken){2, s_byte, i, s_row, s_col, row, i - line_start, err};
            continue;
        }

        i++;
        uint32_t kind = (ch == ';' || ch == ',' || ch == '.' || ch == ':' || ch == '?') ? 2 : 3;
        out_tokens[count++] = (CToken){kind, s_byte, i, s_row, s_col, row, i - line_start, 0};
    }

    if (sp > 0 && *out_delim_err_line == 0) {
        *out_delim_err_line = (int32_t)stack_row[sp - 1];
        *out_delim_err_col = (int32_t)stack_col[sp - 1];
    }

    return (int32_t)count;
}
