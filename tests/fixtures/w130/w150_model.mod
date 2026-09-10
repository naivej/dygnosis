// inventory: w150_model_opt
var x;
varexo e;
parameters betta;
betta = 0.99;
model(linear, bytecode);
x = betta * x(-1) + e;
end;
