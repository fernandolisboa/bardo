---
id: app-credentials
title: Credenciais de app das redes
group: publishing
place: settings/networks
tour: networks
---

# Credenciais de app das redes

Para enviar a uma rede e ler os números dela, o Bardo entra nela com um app que você registra nessa rede. [Configurações › Redes](bardo:go/settings/networks) guarda as credenciais desses apps. [Mostre a aba para mim](bardo:tour/networks).

<a id="networks"></a>
## Um app por rede

| Rede | O app | O que o Bardo pede |
| --- | --- | --- |
| YouTube | Um cliente OAuth do Google do tipo App para computador, no seu projeto do Google Cloud | ID do cliente e chave secreta do cliente |
| Instagram Reels | Um app do tipo Empresa na sua conta de desenvolvedor da Meta, com o Login do Facebook para Empresas | ID do app e chave secreta do app |
| TikTok | Um app (ou o sandbox dele) na sua conta do TikTok for Developers | Client key e client secret |

X e Kick não têm cartão: o Bardo exporta os posts deles para você postar à mão. A linha embaixo do nome da rede, em cada cartão, diz do que o app precisa; o guia de cada rede mostra o caminho passo a passo.

<a id="why"></a>
## Por que o Bardo não traz nenhum

O Bardo não tem app próprio em nenhuma rede. O seu app é registrado por você, então a cota, a tela de consentimento e qualquer revisão ou auditoria são suas, no seu tempo, e o uso do Bardo por outras pessoas não gasta a sua cota. Seus canais ficam entre você e a rede.

Configurar um app leva alguns minutos por rede, uma vez só:

- [Conectar o YouTube](connect-youtube.md)
- [Conectar o Instagram](connect-instagram.md)
- [Conectar o TikTok](connect-tiktok.md)

<a id="save"></a>
## Salvar um app

Cole o ID do app (o TikTok chama de client key) e a chave secreta do site de desenvolvedores da rede no cartão dela e escolha **Salvar**. O Bardo confere o formato e diz embaixo do campo o que parece errado. **Trocar** salva um par novo por cima do antigo, por exemplo depois de você gerar uma chave secreta nova; **Remover** esquece o par.

Se uma rede deixar de aceitar o ID ou a chave salvos, conectar ou verificar uma conta avisa e aponta de volta para cá.

<a id="kept"></a>
## Onde elas ficam

O ID e a chave secreta vão para o Gerenciador de Credenciais do Windows, na sua conta do Windows, por perfil do Bardo, como as suas [chaves de API](api-keys.md). Eles nunca chegam ao banco de dados, aos logs nem às mensagens de erro do Bardo, e a chave secreta não aparece de novo: o cartão diz **Salvas, chave secreta terminada em** e os quatro últimos caracteres dela.

<a id="connect"></a>
## Depois, conecte

Com o app salvo, conecte a conta de cada canal em [Contas](bardo:go/accounts). Veja [Conectar uma conta](network-accounts.md#connect).
