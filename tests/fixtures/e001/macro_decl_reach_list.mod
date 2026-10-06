// inventory: e001_macro_decl_reach_list
var y
@#if 1
x
@#endif
;
varexo e;
parameters p;
p = 0.5;
model;
y = x + e + p;
end;
rplot x;
rplot p;
rplot z;
