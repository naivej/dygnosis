// inventory: w140_local_abs
var x;
varexo e;
parameters betta;
betta = 0.99;
model(linear);
# zloc = abs(x);
x = betta * x(-1) + e;
end;
