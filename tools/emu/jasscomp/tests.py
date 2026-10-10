"""All JASS texts used for the bytecode study (each compiled by the ORIGINAL compiler in Unicorn)."""
HDR = "function f takes nothing returns nothing\n"
IF = "if a == 1 then\nset a = 2\nendif\n"
TESTS = {
 # --- baseline and integer arithmetic
 "00_empty": HDR + "endfunction\n",
 "01_int_add": HDR + "local integer a = 1 + 2\nendfunction\n",
 "02_int_prec": HDR + "local integer a = 1 + 2 * 3\nendfunction\n",
 "03_int_sub_div": HDR + "local integer a = 7 - 4\nlocal integer b = 8 / 2\nendfunction\n",
 "04_int_mod_ERROR": HDR + "local integer a = 7 % 4\nendfunction\n",
 "05_neg_literal": HDR + "local integer a = -5\nendfunction\n",
 "34_neg_var": HDR + "local integer a = 2\nset a = -a\nendfunction\n",
 # --- reals
 "06_real_add": HDR + "local real a = 1.5 + 2.25\nendfunction\n",
 "07_real_mul": HDR + "local real a = 1.5 * 2.0\nendfunction\n",
 "23_real_div": HDR + "local real r = 3.0 / 2.0\nendfunction\n",
 "15_real_literal": HDR + "local real a = 1.5\nendfunction\n",
 # --- comparisons and booleans
 "08_lt": HDR + "local boolean a = 1 < 2\nendfunction\n",
 "09_eq": HDR + "local boolean a = 1 == 2\nendfunction\n",
 "42_ne": HDR + "local boolean b = 1 != 2\nendfunction\n",
 "43_le": HDR + "local boolean b = 1 <= 2\nendfunction\n",
 "44_ge": HDR + "local boolean b = 1 >= 2\nendfunction\n",
 "45_gt": HDR + "local boolean b = 1 > 2\nendfunction\n",
 "25_real_lt": HDR + "local boolean b = 1.5 < 2.0\nendfunction\n",
 "10_and_or": HDR + "local boolean a = true and false\nlocal boolean b = true or false\nendfunction\n",
 "11_not": HDR + "local boolean a = not true\nendfunction\n",
 # --- control flow
 "12_if": HDR + "local integer a = 1\n" + IF + "endfunction\n",
 "13_if_elseif_else": HDR + "local integer a = 1\nif a == 1 then\nset a = 2\nelseif a == 3 then\nset a = 4\nelse\nset a = 5\nendif\nendfunction\n",
 "14_loop_exitwhen": HDR + "local integer i = 0\nloop\nset i = i + 1\nexitwhen i == 3\nendloop\nendfunction\n",
 "46_two_ifs_one_fn": HDR + "local integer a = 1\n" + IF + IF + "endfunction\n",
 "47_labels_two_fns": HDR + "local integer a = 1\n" + IF + "endfunction\nfunction g takes nothing returns nothing\nlocal integer a = 1\n" + IF + "endfunction\n",
 # --- variables, arrays, globals
 "15_array_store": HDR + "local integer array arr\nset arr[2] = 5\nendfunction\n",
 "29_array_read": HDR + "local integer array arr\nlocal integer x = arr[1]\nendfunction\n",
 "16_globals_store": "globals\ninteger g = 3\nendglobals\n" + HDR + "set g = 4\nendfunction\n",
 "48_global_read": "globals\ninteger g = 3\nendglobals\n" + HDR + "local integer x = g\nendfunction\n",
 "49_global_array": "globals\ninteger array ga\nendglobals\n" + HDR + "set ga[1] = 2\nendfunction\n",
 "50_constant_global": "globals\nconstant integer K = 9\nendglobals\n" + HDR + "local integer x = K\nendfunction\n",
 "33_var_copy": HDR + "local integer a = 1\nlocal integer b = a\nendfunction\n",
 "51_decl_only": HDR + "local integer a\nendfunction\n",
 "52_locals_three": HDR + "local integer a\nlocal integer b\nlocal integer c\nendfunction\n",
 "53_set_literal": HDR + "local integer a = 1\nset a = 2\nendfunction\n",
 # --- strings, function refs
 "19_string_lit": HDR + "local string s = \"hi\"\nendfunction\n",
 "41_string_dedup": HDR + "local string s = \"hi\"\nlocal string t = \"yo\"\nlocal string u = \"hi\"\nendfunction\n",
 "24_string_concat": HDR + "local string s = \"a\" + \"b\"\nendfunction\n",
 "20_funcref": "function g takes nothing returns nothing\nendfunction\n" + HDR + "local code c = function g\nendfunction\n",
 # --- calls and returns
 "17_call_args": "function g takes integer x, integer y returns nothing\nendfunction\n" + HDR + "call g(1, 2)\nendfunction\n",
 "35_params_return": "function add takes integer a, integer b returns integer\nreturn a + b\nendfunction\n",
 "18_return_literal": "function r takes nothing returns integer\nreturn 7\nendfunction\n",
 "21_call_value": "function g takes integer x returns integer\nreturn x\nendfunction\n" + HDR + "local integer r = g(3)\nendfunction\n",
 "39_call_in_expr": "function g takes nothing returns integer\nreturn 1\nendfunction\n" + HDR + "local integer r = g() + 2\nendfunction\n",
 "38_call_noargs": "function g takes nothing returns nothing\nendfunction\n" + HDR + "call g()\nendfunction\n",
 "36_return_nothing": "function r takes nothing returns nothing\nreturn\nendfunction\n",
 "40_if_call": "function N takes nothing returns nothing\nendfunction\n" + HDR + "local boolean b = true\nif b then\ncall N()\nendif\nendfunction\n",
 # --- errors (negative results)
 "54_ERR_native_keyword": "native N takes nothing returns nothing\n" + HDR + "endfunction\n",
 "55_ok_constant_function": "constant function N takes nothing returns nothing\nendfunction\n",
 "56_ERR_syntax_set": HDR + "set = 1\nendfunction\n",
 "57_ERR_undeclared_call": HDR + "call Undeclared()\nendfunction\n",
}
