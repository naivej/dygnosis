// inventory: e279_external_fn_outside
var y;
varexo e;
parameters rho;
external_function(name=myf);
rho = myf;
model;
y = rho * y(-1) + e;
end;
