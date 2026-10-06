// inventory: e001_macro_decl_for_include
@#for i in 1:1
var y_@{i};
@#endfor
@#for i in []
var bad-name;
@#endfor
var z
@#include "macro_decl_names.inc"
;
model;
y_1 = 0;
z = 0;
end;
