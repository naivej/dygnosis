// inventory: lists_e240_modellocal_lists
// `#foo` is defined inside the model block, so 7.1 knows it as its own
// `modelLocalVariable` type, and a list reports that rather than the
// undeclared sentence.
var y c;
varexo e;
varexo_det ed;
parameters a;
a = 0.5;

model;
#foo = a*y;
y = a*y(-1) + e;
c = y + foo;
end;

forecast foo;
