// inventory: d_ms_native_assign_heads
// Every head whose line 7.1 reads as native MATLAB text, so the missing-semicolon
// pass must not claim it: an undeclared name, a mod-file local, an
// external-function name, and a name declared only *after* the line. All four
// pairs are 7.1-accepted.
var y c k;
varexo e;
parameters alpha beta gamma;
alpha = 0.36;
beta = 0.99;
gamma = 0.5;
model;
c = alpha*y + beta*c(-1) + e;
y = beta*y(-1) + c;
k = y;
end;
initval;
y = 0;
c = 0;
k = 0;
end;
shocks;
var e; stderr 0.1;
end;
#x = 1;
x = 2
zz = 3;
external_function(name=M_helper);
M_helper = 2
M2_helper = 3;
gg = 1
hh = 2;
parameters gg hh;
aaaa = 1
bbbb = 2;
