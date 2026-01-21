;;; .dir-locals.el

;; set env vars from .env file
;; this is necessary for the static build of slang
;; particularly for SLANG_EXTERNAL_DIR
((nil . ((eval . (let* ((project-dir (expand-file-name (locate-dominating-file default-directory ".dir-locals.el")))
                        (env-file (expand-file-name ".env" project-dir)))
                   (when (file-exists-p env-file)
                     (with-temp-buffer
                       (insert-file-contents env-file)
                       (goto-char (point-min))
                       (while (not (eobp))
                         (let ((line (buffer-substring-no-properties
                                      (line-beginning-position)
                                      (line-end-position))))
                           (unless (or (string-empty-p (string-trim line))
                                       (string-prefix-p "#" (string-trim line)))
                             (when (string-match "^\\([^=]+\\)=\\(.*\\)$" line)
                               (let ((key (match-string 1 line))
                                     (value (match-string 2 line)))
                                 (when (string-match "^\"\\(.*\\)\"$" value)
                                   (setq value (match-string 1 value)))
                                 ;; Replace $PWD with project directory, then expand other vars
                                 (setq value (string-replace "$PWD" (directory-file-name project-dir) value))
                                 (setenv key (substitute-in-file-name value))))))
                         (forward-line 1))))))))))
