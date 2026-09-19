// inventory: d_ms_native_heads
// Lines whose head makes 7.1 read them as native MATLAB text: an undeclared or
// later-declared dotted head, a mod-file local, an external-function name, and a
// non-keyword identifier before `(`. 7.1 makes no language claim on any of them,
// so none may produce an Error here however its contents read.
var y c k;
varexo e;
parameters alpha beta;
alpha = 0.36;
beta = 0.99;
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
zzz.prior(shape=beta,
          mean=0.5,
          variance=0.01);
[aaa, bbb].prior(shape=beta,
                 mean=0.5,
                 stdev=0.1);
nosuchparam.options(init=1,
                    init=2);
sbvar_global_identification_check(parameters=[alpha, delta]);
foo.bar(init=1, init=2);
foo(parameters=[alpha, alpha]);
