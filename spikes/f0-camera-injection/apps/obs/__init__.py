"""Injection-gate harnesses (F0 risk-reduction spike).

Each module captures what one consumer application received with the virtual
camera registered; none of them decides whether the gate passed. That judgement
lives in `spikes/tools/analysis/injection.py` and is applied by
`spikes/f0-camera-injection/run_matrix.py`.
"""
