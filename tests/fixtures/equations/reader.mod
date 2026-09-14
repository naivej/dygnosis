// inventory: equations_reader
var y c z;
varexo e;
varexo_det tau;
parameters rho;
rho = 0.5;

model;
[name='euler'] y = rho * y(-1) + c(+1) + e;
# helper = y + 1;
[static] z = 0;
[dynamic] c = tau + foo;
end;
